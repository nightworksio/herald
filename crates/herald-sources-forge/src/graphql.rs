//! An owner's members polled together through GitHub's GraphQL API: one
//! request asks each member's newest releases by tag, and a second asks in
//! full only the members holding one their cursor has not seen (ADR-0030).

use std::collections::BTreeMap;

use herald_core::{Address, Cursor, Identity, Listed, Polled};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Method, Request, Response, SecretText};
use herald_wire::{SourceError, answered};
use jiff::Timestamp;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::forge::{Repository, authorized};
use crate::wire::{ForgeRelease, event};
use crate::{Forge, Token};

/// What GitHub's GraphQL answers are called.
const WHAT: &str = "GitHub's GraphQL answer";

/// The fields of a release the first request asks for: enough to know
/// whether a cursor has seen it.
const TAGGED: &str = "tagName url isDraft";
/// The fields of a release the second request asks for: all an event
/// takes.
const FULL: &str = "tagName name description url isDraft isPrerelease publishedAt";

/// The query asking each of `count` repositories, the `i`th named by the
/// variables `o{i}` and `n{i}`, for its newest `$page` releases, each with
/// `fields`.
fn query(count: usize, fields: &str) -> String {
    let variables = (0..count)
        .map(|index| format!("$o{index}: String!, $n{index}: String!, "))
        .collect::<Vec<_>>()
        .concat();
    let members = (0..count)
        .map(|index| {
            format!(
                " m{index}: repository(owner: $o{index}, name: $n{index}) {{ databaseId releases(first: $page, orderBy: {{field: CREATED_AT, direction: DESC}}) {{ nodes {{ {fields} }} }} }}"
            )
        })
        .collect::<Vec<_>>()
        .concat();
    format!("query({variables}$page: Int!) {{{members} }}")
}

/// The request asking GitHub's GraphQL API at `endpoint` for the newest
/// `page` releases of each of `members`, each with `fields`, reading at
/// most `parse` bytes.
fn request(
    endpoint: &str,
    members: &[&Listed],
    (fields, page, parse): (&str, usize, usize),
) -> Request {
    let mut variables = serde_json::Map::new();
    for (index, member) in members.iter().enumerate() {
        let owner = member.owner();
        let name = member.path.rsplit('/').next().unwrap_or(&member.name);
        variables.insert(format!("o{index}"), json!(owner));
        variables.insert(format!("n{index}"), json!(name));
    }
    variables.insert("page".to_owned(), json!(page));
    let body = json!({ "query": query(members.len(), fields), "variables": variables });
    Request::new(Method::Post, SecretText::plain(endpoint.to_owned()))
        .header("Content-Type", SecretText::plain(herald_core::media::JSON))
        .body(body.to_string())
        .at_most(parse)
}

/// A release, as GitHub's GraphQL API names it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    tag_name: String,
    url: String,
    is_draft: bool,
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    is_prerelease: bool,
    published_at: Option<Timestamp>,
}

/// A repository's releases, as GitHub's GraphQL API answers them.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answered {
    database_id: u64,
    releases: Nodes,
}

#[derive(Deserialize)]
struct Nodes {
    nodes: Vec<Node>,
}

/// GitHub's GraphQL answer: each member's repository by its alias, null
/// where GitHub names none, and the errors it met.
#[derive(Deserialize)]
struct Envelope {
    data: Option<BTreeMap<String, Option<Answered>>>,
    #[serde(default)]
    errors: Vec<Value>,
}

/// Each of `count` members' repositories in `response`, in order, or why
/// none could be read.
fn answers(response: &Response, count: usize, parse: usize) -> Vec<Result<Answered, Unpolled>> {
    let read = answered(WHAT, response)
        .and_then(|()| herald_wire::read::<Envelope>(WHAT, parse, &response.body));
    let mut data = match read {
        Ok(Envelope {
            data: Some(data), ..
        }) => data,
        Ok(Envelope { errors, .. }) => {
            let reason = format!("{WHAT} holds no data: {}", Value::from(errors));
            return (0..count)
                .map(|_| Err(Unpolled::Unreadable(reason.clone())))
                .collect();
        }
        Err(error) => {
            let unpolled = Unpolled::unreadable(error);
            return (0..count).map(|_| Err(unpolled.clone())).collect();
        }
    };
    (0..count)
        .map(|index| {
            data.remove(&format!("m{index}")).flatten().ok_or_else(|| {
                Unpolled::Unreadable(format!(
                    "{WHAT} names no repository for member {index}; the token may not reach it"
                ))
            })
        })
        .collect()
}

/// The identity of `node`, a release of the repository `id`, as an event
/// of it names it, where it is published.
fn identity(id: u64, node: &Node) -> Option<Identity> {
    let host = Address::parse(&node.url)?;
    (!node.is_draft).then(|| Identity::new(format!("{}/{id}/{}", host.host(), node.tag_name)))
}

/// Whether `answered` holds a published release `cursor` has not seen.
fn unseen(answered: &Answered, cursor: Option<&Cursor>) -> bool {
    let Some(cursor) = cursor else {
        return true;
    };
    answered
        .releases
        .nodes
        .iter()
        .filter_map(|node| identity(answered.database_id, node))
        .any(|identity| !cursor.seen.contains(&identity))
}

/// The releases `answered` holds of the member `member`, as events.
fn polled(member: &Listed, answered: Answered) -> Result<Polled, Unpolled> {
    let repository = Repository {
        id: answered.database_id,
        name: member.name.clone(),
    };
    let events = answered
        .releases
        .nodes
        .into_iter()
        .filter_map(|node| {
            event(
                WHAT,
                &repository,
                ForgeRelease {
                    tag: node.tag_name,
                    name: node.name,
                    body: node.description,
                    url: node.url,
                    draft: node.is_draft,
                    prerelease: node.is_prerelease,
                    published: node.published_at,
                },
            )
            .transpose()
        })
        .collect::<Result<Vec<_>, SourceError>>()
        .map_err(Unpolled::unreadable)?;
    Ok(Polled {
        events,
        etag: None,
        last_modified: None,
    })
}

/// Where and how a batch asks GitHub's GraphQL API.
pub(crate) struct Asking<'a> {
    pub(crate) endpoint: &'a str,
    pub(crate) token: Option<&'a Token>,
    pub(crate) page: usize,
    pub(crate) parse: usize,
}

/// What each of `members`, each with its cursor, answers through `http`,
/// in order: nothing for one whose newest releases its cursor has seen,
/// and its releases in full for the rest, asked together in one second
/// request.
pub(crate) async fn polled_together(
    http: &Erased<'_, dyn DynHttp + '_>,
    asking: &Asking<'_>,
    members: &[(&Listed, Option<&Cursor>)],
) -> Vec<Result<Option<Polled>, Unpolled>> {
    let listed: Vec<&Listed> = members.iter().map(|(member, _)| *member).collect();
    let tagged = match ask(http, asking, &listed, TAGGED).await {
        Ok(tagged) => tagged,
        Err(unpolled) => return members.iter().map(|_| Err(unpolled.clone())).collect(),
    };
    let wanted: Vec<usize> = tagged
        .iter()
        .zip(members)
        .enumerate()
        .filter(|(_, (answered, (_, cursor)))| {
            answered
                .as_ref()
                .is_ok_and(|answered| unseen(answered, *cursor))
        })
        .map(|(index, _)| index)
        .collect();
    let mut full: BTreeMap<usize, Result<Answered, Unpolled>> = BTreeMap::new();
    if !wanted.is_empty() {
        let asked: Vec<&Listed> = listed
            .iter()
            .enumerate()
            .filter(|(index, _)| wanted.contains(index))
            .map(|(_, member)| *member)
            .collect();
        let answered = match ask(http, asking, &asked, FULL).await {
            Ok(answered) => answered,
            Err(unpolled) => wanted.iter().map(|_| Err(unpolled.clone())).collect(),
        };
        full.extend(wanted.iter().copied().zip(answered));
    }
    tagged
        .into_iter()
        .zip(listed)
        .enumerate()
        .map(
            |(index, (answered, member))| match (answered, full.remove(&index)) {
                (Err(unpolled), _) => Err(unpolled),
                (Ok(_), None) => Ok(None),
                (Ok(_), Some(answered)) => answered
                    .and_then(|answered| polled(member, answered))
                    .map(Some),
            },
        )
        .collect()
}

/// Each of `members`' newest releases with `fields`, asked in one request
/// through `http`, in order.
async fn ask(
    http: &Erased<'_, dyn DynHttp + '_>,
    asking: &Asking<'_>,
    members: &[&Listed],
    fields: &str,
) -> Result<Vec<Result<Answered, Unpolled>>, Unpolled> {
    let request = authorized(
        Forge::GitHub,
        request(
            asking.endpoint,
            members,
            (fields, asking.page, asking.parse),
        ),
        asking.token,
    );
    let response = http.send(request).await.map_err(Unpolled::unreachable)?;
    Ok(answers(&response, members.len(), asking.parse))
}

#[cfg(test)]
mod tests;
