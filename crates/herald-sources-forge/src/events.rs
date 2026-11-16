//! An owner's events on GitHub, or its activity feed on Forgejo, read once
//! a round to name the members whose releases or tags changed (ADR-0030,
//! ADR-0031, ADR-0037).

use std::collections::BTreeSet;

use herald_core::{EventsRead, Owner, unreserved};
use herald_kinds::{Changed, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _, Method, Request, SecretText};
use herald_wire::{SourceError, answered};
use serde::Deserialize;

use crate::forge::authorized;
use crate::{Forge, Token};

/// What `forge`'s events are called.
const fn what(forge: Forge) -> &'static str {
    match forge {
        Forge::Forgejo => "Forgejo's activity feed",
        Forge::GitHub | Forge::GitLab => "GitHub's events",
    }
}
/// How many events one page of GitHub's asks for: the most GitHub answers.
const GITHUB_PAGE: usize = 100;
/// How many pages of GitHub's events are read back: all GitHub keeps of an
/// owner's events, 300 events.
const GITHUB_PAGES: usize = 3;
/// The activity Forgejo names a release published by.
const PUBLISH_RELEASE: &str = "publish_release";
/// The activity Forgejo names a tag pushed by.
const PUSH_TAG: &str = "push_tag";
/// The event GitHub names a release by.
const RELEASE: &str = "ReleaseEvent";
/// The action of a release being published.
const PUBLISHED: &str = "published";
/// The event GitHub names a new branch or tag by.
const CREATE: &str = "CreateEvent";
/// What a `CreateEvent` names where it made a tag.
const TAG: &str = "tag";

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
    ref_type: Option<String>,
}

/// One activity, as Forgejo's feed lists it.
#[derive(Deserialize)]
struct Activity {
    id: u64,
    op_type: String,
    repo_id: u64,
}

/// Where and how an owner's events are asked.
pub(crate) struct Asking<'a> {
    pub(crate) forge: Forge,
    pub(crate) paged: (usize, usize),
    pub(crate) api: &'a str,
    pub(crate) name: &'a str,
    pub(crate) owner: Owner,
    pub(crate) token: Option<&'a Token>,
    pub(crate) parse: usize,
    pub(crate) tags: bool,
}

/// How many events a page of GitHub's asks for, and how many pages are read
/// back.
pub(crate) const GITHUB_PAGED: (usize, usize) = (GITHUB_PAGE, GITHUB_PAGES);

impl Asking<'_> {
    /// The request for page `page` of the owner's events, conditional on
    /// `etag` where there is one.
    fn request(&self, page: usize, etag: Option<&str>) -> Request {
        let (api, name, size) = (self.api, unreserved(self.name), self.paged.0);
        let kind = match self.owner {
            Owner::Organization => "orgs",
            Owner::User => "users",
        };
        let url = match self.forge {
            Forge::Forgejo => {
                format!("{api}/{kind}/{name}/activities/feeds?limit={size}&page={page}")
            }
            Forge::GitHub | Forge::GitLab => {
                format!("{api}/{kind}/{name}/events?per_page={size}&page={page}")
            }
        };
        let request = Request::new(Method::Get, SecretText::plain(url))
            .at_most(self.parse)
            .conditional(etag, None);
        authorized(self.forge, request, self.token)
    }

    /// Each of GitHub's events in `body`, by its id, with the member it
    /// names a published release, or a tag where tags are taken, of.
    fn github(&self, body: &[u8]) -> Result<Vec<(u64, Option<String>)>, SourceError> {
        let what = what(self.forge);
        let read: Vec<Happened> = herald_wire::read(what, self.parse, body)?;
        read.into_iter()
            .map(|happened| {
                let id = happened
                    .id
                    .parse::<u64>()
                    .map_err(|error| SourceError::Unreadable {
                        what,
                        reason: format!(
                            "an event's id \"{}\" is not a number: {error}",
                            happened.id
                        ),
                    })?;
                let payload = &happened.payload;
                let released = (happened.kind == RELEASE
                    && payload.action.as_deref() == Some(PUBLISHED))
                    || (self.tags
                        && happened.kind == CREATE
                        && payload.ref_type.as_deref() == Some(TAG));
                Ok((id, released.then(|| happened.repo.id.to_string())))
            })
            .collect()
    }

    /// Each of Forgejo's activities in `body`, by its id, with the member it
    /// names a published release, or a pushed tag where tags are taken, of.
    fn forgejo(&self, body: &[u8]) -> Result<Vec<(u64, Option<String>)>, SourceError> {
        let read: Vec<Activity> = herald_wire::read(what(self.forge), self.parse, body)?;
        Ok(read
            .into_iter()
            .map(|activity| {
                let released = activity.op_type == PUBLISH_RELEASE
                    || (self.tags && activity.op_type == PUSH_TAG);
                (activity.id, released.then(|| activity.repo_id.to_string()))
            })
            .collect())
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
    answered(what(asking.forge), &answer).map_err(Unpolled::unreadable)?;
    let events = match asking.forge {
        Forge::Forgejo => asking.forgejo(&answer.body),
        Forge::GitHub | Forge::GitLab => asking.github(&answer.body),
    }
    .map_err(Unpolled::unreadable)?;
    Ok(Some((events, answer.header("etag").map(ToOwned::to_owned))))
}

/// The members whose releases, or tags where a member may announce them,
/// the owner's events name since `last`, read
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
    let (size, pages) = asking.paged;
    while !reached(&events) && events.len() == number * size && number < pages {
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
