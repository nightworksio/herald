//! An owner's members polled together through GitHub's GraphQL API: one
//! request asks each member's newest releases and tags by name, and a
//! second asks in full only the members holding one their cursor has not
//! seen (ADR-0030, ADR-0031).

use std::collections::BTreeMap;

use herald_core::{Address, Cursor, Event, Identity, Listed, Polled, Signal};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Method, Request, Response, SecretText};
use herald_wire::{SourceError, answered};
use jiff::Timestamp;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::forge::{Repository, authorized};
use crate::wire::{ForgeRelease, ForgeTag, event, tag_event};
use crate::{Forge, Signals, Token};

/// What GitHub's GraphQL answers are called.
const WHAT: &str = "GitHub's GraphQL answer";

/// The fields of a release the first request asks for: enough to know
/// whether a cursor has seen it.
const TAGGED: &str = "tagName url isDraft";
/// The fields of a release the second request asks for: all an event
/// takes.
const FULL: &str = "tagName name description url isDraft isPrerelease publishedAt";
/// The fields of a tag the first request asks for.
const NAMED: &str = "name";
/// The fields of a tag the second request asks for: its message and time,
/// the annotated tag's date or else its commit's.
const DATED: &str = "name target { __typename ... on Tag { message tagger { date } target { ... on Commit { committedDate } } } ... on Commit { committedDate } }";

/// Which fields a request asks of each release and tag.
#[derive(Clone, Copy)]
struct Fields {
    release: &'static str,
    tag: &'static str,
}

/// What the first request asks.
const LIGHT: Fields = Fields {
    release: TAGGED,
    tag: NAMED,
};
/// What the second request asks.
const WHOLE: Fields = Fields {
    release: FULL,
    tag: DATED,
};

/// The query asking each repository of `signals`, the `i`th named by the
/// variables `o{i}` and `n{i}`, for its newest `$page` releases and tags,
/// as its signals take them, each with `fields`.
fn query(signals: &[&Signals], fields: Fields) -> String {
    let variables = (0..signals.len())
        .map(|index| format!("$o{index}: String!, $n{index}: String!, "))
        .collect::<Vec<_>>()
        .concat();
    let members = signals
        .iter()
        .enumerate()
        .map(|(index, signals)| {
            let releases = if signals.takes(Signal::Release) {
                format!(
                    " releases(first: $page, orderBy: {{field: CREATED_AT, direction: DESC}}) {{ nodes {{ {} }} }}",
                    fields.release
                )
            } else {
                String::new()
            };
            let tags = if signals.takes(Signal::Tag) {
                format!(
                    " refs(refPrefix: \"refs/tags/\", first: $page, orderBy: {{field: TAG_COMMIT_DATE, direction: DESC}}) {{ nodes {{ {} }} }}",
                    fields.tag
                )
            } else {
                String::new()
            };
            format!(
                " m{index}: repository(owner: $o{index}, name: $n{index}) {{ databaseId url{releases}{tags} }}"
            )
        })
        .collect::<Vec<_>>()
        .concat();
    format!("query({variables}$page: Int!) {{{members} }}")
}

/// The request asking GitHub's GraphQL API at `endpoint` for the newest
/// `page` releases and tags of each of `members`, as its signals take
/// them, each with `fields`, reading at most `parse` bytes.
fn request(
    endpoint: &str,
    members: &[(&Listed, &Signals)],
    (fields, page, parse): (Fields, usize, usize),
) -> Request {
    let mut variables = serde_json::Map::new();
    for (index, (member, _)) in members.iter().enumerate() {
        let owner = member.owner();
        let name = member.path.rsplit('/').next().unwrap_or(&member.name);
        variables.insert(format!("o{index}"), json!(owner));
        variables.insert(format!("n{index}"), json!(name));
    }
    variables.insert("page".to_owned(), json!(page));
    let signals: Vec<&Signals> = members.iter().map(|(_, signals)| *signals).collect();
    let body = json!({ "query": query(&signals, fields), "variables": variables });
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

/// A tag, as GitHub's GraphQL API names it.
#[derive(Deserialize)]
struct Ref {
    name: String,
    target: Option<Target>,
}

/// What a tag points at: an annotated tag object, or a commit.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    message: Option<String>,
    tagger: Option<Tagger>,
    #[serde(rename = "target")]
    pointed: Option<Box<Target>>,
    committed_date: Option<Timestamp>,
}

#[derive(Deserialize)]
struct Tagger {
    date: Option<Timestamp>,
}

impl Ref {
    /// The tag, read in full: an annotated tag's message and date, or a
    /// lightweight tag's commit's date.
    fn read(self) -> ForgeTag {
        let target = self.target;
        let tagged = target
            .as_ref()
            .and_then(|target| target.tagger.as_ref())
            .and_then(|tagger| tagger.date);
        let committed = target.as_ref().and_then(|target| {
            target.committed_date.or_else(|| {
                target
                    .pointed
                    .as_ref()
                    .and_then(|commit| commit.committed_date)
            })
        });
        ForgeTag {
            name: self.name,
            message: target
                .and_then(|target| target.message)
                .filter(|message| !message.trim().is_empty()),
            date: tagged.or(committed),
        }
    }
}

/// A repository's releases and tags, as GitHub's GraphQL API answers them.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answered {
    database_id: u64,
    url: Option<String>,
    releases: Option<Nodes<Node>>,
    refs: Option<Nodes<Ref>>,
}

#[derive(Deserialize)]
struct Nodes<T> {
    nodes: Vec<T>,
}

impl Answered {
    fn releases(&self) -> &[Node] {
        self.releases
            .as_ref()
            .map_or(&[], |releases| &releases.nodes)
    }

    fn tags(&self) -> &[Ref] {
        self.refs.as_ref().map_or(&[], |refs| &refs.nodes)
    }

    /// The host of the repository's page, where GitHub names one.
    fn host(&self) -> Option<String> {
        self.url
            .as_deref()
            .and_then(Address::parse)
            .map(|address| address.host().to_owned())
    }
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
    (!node.is_draft).then(|| crate::wire::identity(host.host(), id, &node.tag_name))
}

/// Whether `answered` holds a published release or a tag `cursor` has not
/// seen.
fn unseen(answered: &Answered, cursor: Option<&Cursor>) -> bool {
    let Some(cursor) = cursor else {
        return true;
    };
    let host = answered.host();
    let released = answered
        .releases()
        .iter()
        .filter_map(|node| identity(answered.database_id, node));
    let tagged = answered.tags().iter().filter_map(|tag| {
        host.as_deref()
            .map(|host| crate::wire::identity(host, answered.database_id, &tag.name))
    });
    released
        .chain(tagged)
        .any(|identity| !cursor.seen.contains(&identity))
}

/// The releases and tags `answered` holds of the member `member`, as
/// events, newest first, each pre-release told as `signals` say; a tag
/// sharing its identity with a release is the release's.
fn polled(member: &Listed, answered: Answered, signals: &Signals) -> Result<Polled, Unpolled> {
    let host = answered.host();
    let repository = Repository {
        id: answered.database_id,
        name: member.name.clone(),
        page: answered.url.clone(),
    };
    let rule = signals.prerelease;
    let mut events = answered
        .releases
        .into_iter()
        .flat_map(|releases| releases.nodes)
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
                    prerelease: Some(node.is_prerelease),
                    published: node.published_at,
                },
                rule,
            )
            .transpose()
        })
        .collect::<Result<Vec<Event>, SourceError>>()
        .map_err(Unpolled::unreadable)?;
    let tags: Vec<Ref> = answered
        .refs
        .into_iter()
        .flat_map(|refs| refs.nodes)
        .collect();
    if let (Some(host), Some(page)) = (host, &repository.page) {
        for tag in tags {
            let identity = crate::wire::identity(&host, repository.id, &tag.name);
            if events.iter().any(|event| event.identity() == &identity) {
                continue;
            }
            let url = Forge::GitHub.tag_page(page, &tag.name);
            events.push(
                tag_event(WHAT, &repository, (tag.read(), url), rule)
                    .map_err(Unpolled::unreadable)?,
            );
        }
    } else if !tags.is_empty() {
        return Err(Unpolled::Unreadable(format!(
            "{WHAT} names no web page of {}, which its tags are announced by",
            member.path
        )));
    }
    events.sort_by_key(|event| std::cmp::Reverse(event.occurred()));
    Ok(Polled {
        events,
        ..Polled::default()
    })
}

/// Where and how a batch asks GitHub's GraphQL API.
pub(crate) struct Asking<'a> {
    pub(crate) endpoint: &'a str,
    pub(crate) token: Option<&'a Token>,
    pub(crate) page: usize,
    pub(crate) parse: usize,
}

/// What each of `members`, each with its cursor and signals, answers
/// through `http`, in order: nothing for one whose newest releases and
/// tags its cursor has seen, and its releases and tags in full for the
/// rest, asked together in one second request.
pub(crate) async fn polled_together(
    http: &Erased<'_, dyn DynHttp + '_>,
    asking: &Asking<'_>,
    members: &[(&Listed, Option<&Cursor>, Signals)],
) -> Vec<Result<Option<Polled>, Unpolled>> {
    let listed: Vec<(&Listed, &Signals)> = members
        .iter()
        .map(|(member, _, signals)| (*member, signals))
        .collect();
    let tagged = match ask(http, asking, &listed, LIGHT).await {
        Ok(tagged) => tagged,
        Err(unpolled) => return members.iter().map(|_| Err(unpolled.clone())).collect(),
    };
    let wanted: Vec<usize> = tagged
        .iter()
        .zip(members)
        .enumerate()
        .filter(|(_, (answered, (_, cursor, _)))| {
            answered
                .as_ref()
                .is_ok_and(|answered| unseen(answered, *cursor))
        })
        .map(|(index, _)| index)
        .collect();
    let mut full: BTreeMap<usize, Result<Answered, Unpolled>> = BTreeMap::new();
    if !wanted.is_empty() {
        let asked: Vec<(&Listed, &Signals)> = listed
            .iter()
            .enumerate()
            .filter(|(index, _)| wanted.contains(index))
            .map(|(_, member)| *member)
            .collect();
        let answered = match ask(http, asking, &asked, WHOLE).await {
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
            |(index, (answered, (member, signals)))| match (answered, full.remove(&index)) {
                (Err(unpolled), _) => Err(unpolled),
                (Ok(_), None) => Ok(None),
                (Ok(_), Some(answered)) => answered
                    .and_then(|answered| polled(member, answered, signals))
                    .map(Some),
            },
        )
        .collect()
}

/// Each of `members`' newest releases and tags with `fields`, asked in one
/// request through `http`, in order.
async fn ask(
    http: &Erased<'_, dyn DynHttp + '_>,
    asking: &Asking<'_>,
    members: &[(&Listed, &Signals)],
    fields: Fields,
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
