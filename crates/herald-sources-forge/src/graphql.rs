//! An owner's members polled together through GitHub's or GitLab's
//! GraphQL API: one request asks each member's newest releases (and on
//! GitHub its tags), and a second asks in full only the members holding
//! one their cursor has not seen (ADR-0030, ADR-0031, ADR-0037).

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

/// What `forge`'s GraphQL answers are called.
const fn what(forge: Forge) -> &'static str {
    match forge {
        Forge::GitLab => "GitLab's GraphQL answer",
        Forge::GitHub | Forge::Forgejo => "GitHub's GraphQL answer",
    }
}

/// The fields of a GitLab release the first request asks for.
const GITLAB_TAGGED: &str = "tagName upcomingRelease links { selfUrl }";
/// The fields of a GitLab release the second request asks for.
const GITLAB_FULL: &str = "tagName name description releasedAt upcomingRelease links { selfUrl }";

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

/// Which fields a request asks of each release and tag on GitHub, and of
/// each release on GitLab.
#[derive(Clone, Copy)]
struct Fields {
    release: &'static str,
    tag: &'static str,
    gitlab: &'static str,
}

/// What the first request asks.
const LIGHT: Fields = Fields {
    release: TAGGED,
    tag: NAMED,
    gitlab: GITLAB_TAGGED,
};
/// What the first request asks where a poll watches Releases for an edit:
/// each Release in full, to tell its name and notes apart.
const WATCHING: Fields = Fields {
    release: FULL,
    tag: NAMED,
    gitlab: GITLAB_FULL,
};
/// What the second request asks.
const WHOLE: Fields = Fields {
    release: FULL,
    tag: DATED,
    gitlab: GITLAB_FULL,
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
    posted(endpoint, &body, parse)
}

/// The request asking GitLab's GraphQL API at `endpoint` for the newest
/// `page` releases of each of `members`, by its project id, each with
/// `fields`, reading at most `parse` bytes.
fn gitlab_request(
    endpoint: &str,
    members: &[(&Listed, &Signals)],
    (fields, page, parse): (Fields, usize, usize),
) -> Request {
    let ids: Vec<String> = members
        .iter()
        .map(|(member, _)| format!("{GITLAB_PROJECT}{}", member.id))
        .collect();
    let query = format!(
        "query($ids: [ID!], $page: Int!) {{ projects(ids: $ids, first: {GITLAB_MOST}) {{ nodes {{ id webUrl releases(first: $page, sort: RELEASED_AT_DESC) {{ nodes {{ {} }} }} }} }} }}",
        fields.gitlab
    );
    let body = json!({ "query": query, "variables": { "ids": ids, "page": page } });
    posted(endpoint, &body, parse)
}

/// What a GitLab project's global id is written after.
const GITLAB_PROJECT: &str = "gid://gitlab/Project/";
/// The most projects one GitLab request lists, as many as a batch holds.
const GITLAB_MOST: usize = crate::options::GRAPHQL_BATCH_ALLOWED;

/// `body` posted as JSON to `endpoint`, reading at most `parse` bytes.
fn posted(endpoint: &str, body: &Value, parse: usize) -> Request {
    Request::new(Method::Post, SecretText::plain(endpoint.to_owned()))
        .header("Content-Type", SecretText::plain(herald_core::media::JSON))
        .body(body.to_string())
        .at_most(parse)
}

/// A release, as GitHub's GraphQL API names it.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    tag_name: String,
    url: String,
    is_draft: bool,
    name: Option<String>,
    description: Option<String>,
    is_prerelease: Option<bool>,
    published_at: Option<Timestamp>,
}

/// A release, as GitLab's GraphQL API names it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitLabRelease {
    tag_name: String,
    name: Option<String>,
    description: Option<String>,
    released_at: Option<Timestamp>,
    #[serde(default)]
    upcoming_release: bool,
    links: GitLabLinks,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitLabLinks {
    self_url: String,
}

impl From<GitLabRelease> for Node {
    fn from(release: GitLabRelease) -> Self {
        Self {
            tag_name: release.tag_name,
            url: release.links.self_url,
            is_draft: release.upcoming_release,
            name: release.name,
            description: release.description,
            is_prerelease: None,
            published_at: release.released_at,
        }
    }
}

/// A project and its releases, as GitLab's GraphQL API answers them.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitLabProject {
    id: String,
    web_url: Option<String>,
    releases: Option<Nodes<GitLabRelease>>,
}

/// The projects of GitLab's GraphQL answer.
#[derive(Deserialize)]
struct GitLabProjects {
    projects: Nodes<GitLabProject>,
}

/// GitLab's GraphQL answer: the projects it names, and the errors it met.
#[derive(Deserialize)]
struct GitLabEnvelope {
    data: Option<GitLabProjects>,
    #[serde(default)]
    errors: Vec<Value>,
}

/// A tag, as GitHub's GraphQL API names it.
#[derive(Clone, Deserialize)]
struct Ref {
    name: String,
    target: Option<Target>,
}

/// What a tag points at: an annotated tag object, or a commit.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    message: Option<String>,
    tagger: Option<Tagger>,
    #[serde(rename = "target")]
    pointed: Option<Box<Target>>,
    committed_date: Option<Timestamp>,
}

#[derive(Clone, Deserialize)]
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
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answered {
    database_id: u64,
    url: Option<String>,
    releases: Option<Nodes<Node>>,
    refs: Option<Nodes<Ref>>,
}

#[derive(Clone, Deserialize)]
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

/// Each of `members`' repositories in `response` from `forge`, in order,
/// or why none could be read.
fn answers(
    forge: Forge,
    response: &Response,
    members: &[(&Listed, &Signals)],
    parse: usize,
) -> Vec<Result<Answered, Unpolled>> {
    let what = what(forge);
    let read = answered(what, response)
        .map_err(Unpolled::unreadable)
        .and_then(|()| match forge {
            Forge::GitLab => gitlab_answers(what, parse, &response.body),
            Forge::GitHub | Forge::Forgejo => github_answers(what, parse, &response.body),
        });
    let mut found = match read {
        Ok(found) => found,
        Err(unpolled) => return members.iter().map(|_| Err(unpolled.clone())).collect(),
    };
    members
        .iter()
        .enumerate()
        .map(|(index, (member, _))| {
            let key = match forge {
                Forge::GitLab => member.id.clone(),
                Forge::GitHub | Forge::Forgejo => format!("m{index}"),
            };
            found.remove(&key).ok_or_else(|| {
                Unpolled::Unreadable(format!(
                    "{what} names no repository for member {index}; the token may not reach it"
                ))
            })
        })
        .collect()
}

/// Each repository GitHub's GraphQL answer `body` names, by its alias.
fn github_answers(
    what: &'static str,
    parse: usize,
    body: &[u8],
) -> Result<BTreeMap<String, Answered>, Unpolled> {
    match herald_wire::read::<Envelope>(what, parse, body).map_err(Unpolled::unreadable)? {
        Envelope {
            data: Some(data), ..
        } => Ok(data
            .into_iter()
            .filter_map(|(alias, answered)| Some((alias, answered?)))
            .collect()),
        Envelope { errors, .. } => Err(empty(what, errors)),
    }
}

/// Each project GitLab's GraphQL answer `body` names, by its id.
fn gitlab_answers(
    what: &'static str,
    parse: usize,
    body: &[u8],
) -> Result<BTreeMap<String, Answered>, Unpolled> {
    match herald_wire::read::<GitLabEnvelope>(what, parse, body).map_err(Unpolled::unreadable)? {
        GitLabEnvelope {
            data: Some(data), ..
        } => Ok(data
            .projects
            .nodes
            .into_iter()
            .filter_map(|project| {
                let id = project.id.strip_prefix(GITLAB_PROJECT)?.to_owned();
                let answered = Answered {
                    database_id: id.parse().ok()?,
                    url: project.web_url,
                    releases: project.releases.map(|releases| Nodes {
                        nodes: releases.nodes.into_iter().map(Node::from).collect(),
                    }),
                    refs: None,
                };
                Some((id, answered))
            })
            .collect()),
        GitLabEnvelope { errors, .. } => Err(empty(what, errors)),
    }
}

/// That `what` holds no data, beside the `errors` it names.
fn empty(what: &str, errors: Vec<Value>) -> Unpolled {
    Unpolled::Unreadable(format!("{what} holds no data: {}", Value::from(errors)))
}

/// The identity of `node`, a release of the repository `id`, as an event
/// of it names it, where it is published.
fn identity(id: u64, node: &Node) -> Option<Identity> {
    let host = Address::parse(&node.url)?;
    (!node.is_draft).then(|| crate::wire::identity(host.host(), id, &node.tag_name))
}

/// Whether `answered` holds a published release or a tag `cursor` has not
/// seen, or a release of a version it has seen only as a tag.
fn unseen(answered: &Answered, cursor: Option<&Cursor>) -> bool {
    let Some(cursor) = cursor else {
        return true;
    };
    let host = answered.host();
    let released = answered
        .releases()
        .iter()
        .filter_map(|node| identity(answered.database_id, node))
        .any(|identity| !cursor.seen.contains(&identity) || cursor.tagged.contains(&identity));
    released
        || answered.tags().iter().any(|tag| {
            host.as_deref().is_some_and(|host| {
                !cursor.seen.contains(&crate::wire::identity(
                    host,
                    answered.database_id,
                    &tag.name,
                ))
            })
        })
}

/// Whether a Release `answered` holds of `member`, told as `signals` say,
/// is one `cursor` watches whose name or notes changed.
fn edited(
    (member, signals, forge): (&Listed, &Signals, Forge),
    answered: &Answered,
    cursor: Option<&Cursor>,
) -> bool {
    cursor.is_some_and(|cursor| {
        polled((member, forge), answered.clone(), signals)
            .is_ok_and(|polled| polled.events.iter().any(|event| cursor.edited(event)))
    })
}

/// The releases and tags `answered` holds of the member `member`, as
/// events, newest first, each pre-release told as `signals` say; a tag
/// sharing its identity with a release is the release's.
fn polled(
    (member, forge): (&Listed, Forge),
    answered: Answered,
    signals: &Signals,
) -> Result<Polled, Unpolled> {
    let what = what(forge);
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
                what,
                &repository,
                ForgeRelease {
                    tag: node.tag_name,
                    name: node.name,
                    body: node.description,
                    url: node.url,
                    draft: node.is_draft,
                    prerelease: node.is_prerelease,
                    published: node.published_at,
                    edited: false,
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
                tag_event(what, &repository, (tag.read(), url), rule)
                    .map_err(Unpolled::unreadable)?,
            );
        }
    } else if !tags.is_empty() {
        return Err(Unpolled::Unreadable(format!(
            "{what} names no web page of {}, which its tags are announced by",
            member.path
        )));
    }
    events.sort_by_key(|event| std::cmp::Reverse(event.occurred()));
    Ok(Polled {
        events,
        ..Polled::default()
    })
}

/// Where and how a batch asks a forge's GraphQL API.
pub(crate) struct Asking<'a> {
    pub(crate) forge: Forge,
    pub(crate) endpoint: &'a str,
    pub(crate) token: Option<&'a Token>,
    pub(crate) page: usize,
    pub(crate) parse: usize,
    pub(crate) watched: usize,
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
    let first = if asking.watched > 0 { WATCHING } else { LIGHT };
    let tagged = match ask(http, asking, &listed, first).await {
        Ok(tagged) => tagged,
        Err(unpolled) => return members.iter().map(|_| Err(unpolled.clone())).collect(),
    };
    let wanted: Vec<usize> = tagged
        .iter()
        .zip(members)
        .enumerate()
        .filter(|(_, (answered, (member, cursor, signals)))| {
            answered.as_ref().is_ok_and(|answered| {
                unseen(answered, *cursor)
                    || (asking.watched > 0
                        && edited((member, signals, asking.forge), answered, *cursor))
            })
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
                    .and_then(|answered| polled((member, asking.forge), answered, signals))
                    .map(|polled| {
                        Some(Polled {
                            watched: asking.watched,
                            ..polled
                        })
                    }),
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
    let reading = (fields, asking.page, asking.parse);
    let asked = match asking.forge {
        Forge::GitLab => gitlab_request(asking.endpoint, members, reading),
        Forge::GitHub | Forge::Forgejo => request(asking.endpoint, members, reading),
    };
    let request = authorized(asking.forge, asked, asking.token);
    let response = http.send(request).await.map_err(Unpolled::unreachable)?;
    Ok(answers(asking.forge, &response, members, asking.parse))
}

#[cfg(test)]
mod tests;
