//! An owner's events on GitHub, read once a round to name the members whose
//! releases changed (ADR-0030).

use std::collections::BTreeSet;

use herald_core::{EventsRead, Owner, unreserved};
use herald_kinds::{Changed, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _, Method, Request, SecretText};
use herald_wire::{SourceError, answered};
use serde::Deserialize;

use crate::forge::authorized;
use crate::{Forge, Token};

/// What GitHub's events are called.
const WHAT: &str = "GitHub's events";
/// How many events one page asks for: the most GitHub answers.
const PER_PAGE: usize = 100;
/// How many pages GitHub keeps of an owner's events: 300 events.
const PAGES: usize = 3;
/// The event GitHub names a release by.
const RELEASE: &str = "ReleaseEvent";
/// The action of a release being published.
const PUBLISHED: &str = "published";

/// One event, as GitHub lists it.
#[derive(Deserialize)]
struct Happened {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    repo: Repo,
    #[serde(default)]
    payload: Payload,
}

#[derive(Deserialize)]
struct Repo {
    id: u64,
}

#[derive(Deserialize, Default)]
struct Payload {
    action: Option<String>,
}

/// Where and how an owner's events are asked.
pub(crate) struct Asking<'a> {
    pub(crate) api: &'a str,
    pub(crate) name: &'a str,
    pub(crate) owner: Owner,
    pub(crate) token: Option<&'a Token>,
    pub(crate) parse: usize,
}

impl Asking<'_> {
    /// The request for page `page` of the owner's events, conditional on
    /// `etag` where there is one.
    fn request(&self, page: usize, etag: Option<&str>) -> Request {
        let (api, name) = (self.api, unreserved(self.name));
        let url = match self.owner {
            Owner::Organization => {
                format!("{api}/orgs/{name}/events?per_page={PER_PAGE}&page={page}")
            }
            Owner::User => format!("{api}/users/{name}/events?per_page={PER_PAGE}&page={page}"),
        };
        let request = Request::new(Method::Get, SecretText::plain(url))
            .at_most(self.parse)
            .conditional(etag, None);
        authorized(Forge::GitHub, request, self.token)
    }
}

/// One page of events: each event's id, newest first, with the member it
/// names a published release of, and the page's `ETag`.
type Page = (Vec<(u64, Option<String>)>, Option<String>);

/// The events on one page, or nothing where it is unchanged since `etag`.
async fn page(
    http: &Erased<'_, dyn DynHttp + '_>,
    asking: &Asking<'_>,
    (number, etag): (usize, Option<&str>),
) -> Result<Option<Page>, Unpolled> {
    let answer = http
        .send(asking.request(number, etag))
        .await
        .map_err(Unpolled::unreachable)?;
    if etag.is_some() && Forge::unchanged(&answer) {
        return Ok(None);
    }
    answered(WHAT, &answer).map_err(Unpolled::unreadable)?;
    let read: Vec<Happened> =
        herald_wire::read(WHAT, asking.parse, &answer.body).map_err(Unpolled::unreadable)?;
    let events = read
        .into_iter()
        .map(|happened| {
            let id = happened
                .id
                .parse::<u64>()
                .map_err(|error| SourceError::Unreadable {
                    what: WHAT,
                    reason: format!("an event's id \"{}\" is not a number: {error}", happened.id),
                })?;
            let released =
                happened.kind == RELEASE && happened.payload.action.as_deref() == Some(PUBLISHED);
            Ok((id, released.then(|| happened.repo.id.to_string())))
        })
        .collect::<Result<Vec<_>, SourceError>>()
        .map_err(Unpolled::unreadable)?;
    Ok(Some((events, answer.header("etag").map(ToOwned::to_owned))))
}

/// The members whose releases the owner's events name since `last`, read
/// conditionally on its `ETag`: every page GitHub keeps is read until one
/// reaches back to `last`, and where none does, or there is no `last`,
/// every member is polled.
pub(crate) async fn changed(
    http: &Erased<'_, dyn DynHttp + '_>,
    asking: &Asking<'_>,
    last: Option<&EventsRead>,
) -> Result<Changed, Unpolled> {
    let Some((first, etag)) = page(
        http,
        asking,
        (1, last.and_then(|last| last.etag.as_deref())),
    )
    .await?
    else {
        let read = last.cloned().unwrap_or_default();
        return Ok(Changed::Named {
            ids: BTreeSet::new(),
            read,
        });
    };
    let newest = first
        .first()
        .map_or(last.map_or(0, |last| last.newest), |(id, _)| *id);
    let read = EventsRead { etag, newest };
    let Some(last) = last else {
        return Ok(Changed::Gap(read));
    };
    let mut events = first;
    let mut number = 1;
    let reached =
        |events: &[(u64, Option<String>)]| events.iter().any(|(id, _)| *id <= last.newest);
    while !reached(&events) && events.len() == number * PER_PAGE && number < PAGES {
        number += 1;
        let more = page(http, asking, (number, None)).await?;
        events.extend(more.into_iter().flat_map(|(more, _)| more));
    }
    if !reached(&events) {
        return Ok(Changed::Gap(read));
    }
    let ids = events
        .into_iter()
        .filter(|(id, _)| *id > last.newest)
        .filter_map(|(_, released)| released)
        .collect();
    Ok(Changed::Named { ids, read })
}

#[cfg(test)]
mod tests;
