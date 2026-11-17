use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use herald_core::{
    Attribute, Cursor, Draft, Event, Identity, Kind, Origin, PACKAGE, PACKAGE_TYPE, Polled,
    Problem, RELEASE_LINK, Visibility, prerelease_version, status, unreserved, versioned,
};
use herald_kinds::{Poll, Probed, Unpolled};
use herald_ports::{Boxed, DynHttp, Response};
use herald_wire::{SourceError, answered};
use jiff::{SignedDuration, Timestamp};

use super::session::{Blob, Failure, Reached, Session};
use super::wire::{self, Manifested};
use crate::docker_hub;

/// How far past the round's time a tag's time may fall before it is not
/// believed, as whoever pushes an image writes its `created` and the
/// registry its `Date`.
const AHEAD: SignedDuration = SignedDuration::from_mins(5);

/// The kind of package a container image is.
const CONTAINER: &str = "container";

/// One repository of a registry, whose tags are its versions.
#[derive(Debug)]
pub(crate) struct Image {
    pub(crate) reached: Arc<Reached>,
    pub(crate) name: String,
    pub(crate) visibility: Visibility,
}

/// What reading a tag's manifest found of when its image was created.
#[derive(Debug, Default)]
struct Dated {
    /// The time the image says it was created, past the epoch.
    created: Option<Timestamp>,
    /// When the registry answered for the tag.
    seen: Option<Timestamp>,
    /// Where its config blob was redirected that the source may not reach.
    redirected: Option<String>,
}

/// One tag's listing: each tag, and when the registry answered it.
struct Listed {
    tags: Vec<String>,
    answered: Option<Timestamp>,
}

impl Image {
    /// The image `name` of `reached`, seen as `visibility`.
    pub(crate) fn new(reached: Arc<Reached>, name: String, visibility: Visibility) -> Self {
        Self {
            reached,
            name,
            visibility,
        }
    }

    fn session<'a>(&'a self, http: &'a dyn DynHttp) -> Session<'a> {
        Session::new(
            &self.reached,
            http,
            format!("repository:{}:pull", self.name),
        )
    }

    /// The name its events give as their project: the repository's, without
    /// Docker Hub's `library/` for an official image.
    pub(crate) fn project(&self) -> &str {
        if self.reached.docker_hub {
            docker_hub::project(&self.name)
        } else {
            &self.name
        }
    }

    /// Where its events come from: the registry, the repository's owner,
    /// and the repository, seen as its visibility.
    pub(crate) fn origin(&self) -> Origin {
        let owner = self.name.rsplit_once('/').map_or("", |(owner, _)| owner);
        Origin {
            visibility: self.visibility,
            ..Origin::of_package(super::KIND, &self.reached.name, owner, &self.name)
        }
    }

    fn identity(&self, tag: &str) -> Identity {
        Identity::new(format!(
            "{}/{}/{tag}",
            self.reached.name,
            self.name.to_ascii_lowercase()
        ))
    }

    /// Every tag the repository lists, page by page, those that move left
    /// out.
    async fn listed(&self, session: &mut Session<'_>) -> Result<Listed, Failure> {
        let reading = self.reached.reading;
        let page = self.reached.listed_page;
        let mut tags: Vec<String> = Vec::new();
        let mut known = BTreeSet::new();
        let mut answered_at = None;
        loop {
            let last = tags
                .last()
                .map(|last| format!("&last={}", unreserved(last)))
                .unwrap_or_default();
            let path = format!("/v2/{}/tags/list?n={page}{last}", self.name);
            let request = session.get(&path).accepting(herald_core::media::JSON);
            let answer = session.send(request).await?;
            answered(wire::TAGS, &answer).map_err(|error| Failure::read("repository", &error))?;
            answered_at = answered_at.or_else(|| answer.date());
            let listed = wire::tags(reading.parse, &answer.body)
                .map_err(|error| Failure::read("repository", &error))?;
            if let Some(repeated) = listed.iter().find(|tag| known.contains(*tag)) {
                return Err(Failure::at(
                    "registry",
                    format!(
                        "the registry listed {}'s tag \"{repeated}\" twice; it does not page its tags by last",
                        self.name
                    ),
                ));
            }
            let more =
                !listed.is_empty() && (wire::next(answer.header("link")) || listed.len() >= page);
            known.extend(listed.iter().cloned());
            tags.extend(listed);
            if tags.len() > self.reached.most_tags {
                return Err(Failure::at(
                    "most_tags",
                    format!(
                        "{} lists more than {} tags, most_tags; raise it, or name another repository",
                        self.name, self.reached.most_tags
                    ),
                ));
            }
            if !more {
                break;
            }
        }
        tags.retain(|tag| !self.reached.moving.iter().any(|glob| glob.matches(tag)));
        Ok(Listed {
            tags,
            answered: answered_at,
        })
    }

    fn manifest(&self, session: &Session<'_>, reference: &str) -> herald_ports::Request {
        session
            .get(&format!("/v2/{}/manifests/{reference}", self.name))
            .accepting(wire::MANIFESTS)
    }

    /// When the image `tag` names was created, as its manifest, its first
    /// platform's manifest or its config says; nothing where the tag is
    /// gone.
    async fn dated(&self, session: &mut Session<'_>, tag: &str) -> Result<Option<Dated>, Failure> {
        let answer = session.send(self.manifest(session, tag)).await?;
        if answer.status == status::NOT_FOUND {
            return Ok(None);
        }
        let mut dated = Dated {
            seen: answer.date(),
            ..Dated::default()
        };
        let mut manifested = self.manifested(&answer)?;
        if let Manifested::Platform(digest) = &manifested {
            let answer = session.send(self.manifest(session, digest)).await?;
            manifested = self.manifested(&answer)?;
        }
        match manifested {
            Manifested::Created(created) => dated.created = Some(created),
            Manifested::Platform(_) => {}
            Manifested::Config(digest) => {
                let request = session.get(&format!("/v2/{}/blobs/{digest}", self.name));
                match session.blob(request).await? {
                    Blob::Read(answer) => {
                        answered(wire::CONFIG, &answer)
                            .and_then(|()| wire::created(self.reached.reading.parse, &answer.body))
                            .map(|created| dated.created = created)
                            .map_err(|error| Failure::read("repository", &error))?;
                    }
                    Blob::Redirected(origin) => dated.redirected = Some(origin),
                }
            }
        }
        Ok(Some(dated))
    }

    fn manifested(&self, answer: &Response) -> Result<Manifested, Failure> {
        answered(wire::MANIFEST, answer)
            .and_then(|()| wire::manifested(self.reached.reading.parse, &answer.body))
            .map_err(|error| Failure::read("repository", &error))
    }

    async fn poll(
        &self,
        http: &dyn DynHttp,
        cursor: Option<&Cursor>,
        now: Timestamp,
    ) -> Result<Polled, Failure> {
        let mut session = self.session(http);
        let listed = self.listed(&mut session).await?;
        let seen: BTreeSet<&Identity> = cursor.iter().flat_map(|cursor| &cursor.seen).collect();
        let identified: Vec<(Identity, &String)> = listed
            .tags
            .iter()
            .map(|tag| (self.identity(tag), tag))
            .collect();
        let first = cursor.is_none_or(|cursor| cursor.seen.is_empty());
        let unseen: Vec<&(Identity, &String)> = identified
            .iter()
            .filter(|(identity, _)| !seen.contains(identity))
            .collect();
        let page = self.reached.reading.page;
        let reads = if cursor.is_none() && self.reached.backfill == 0 {
            0
        } else {
            page
        };
        let read: Vec<&(Identity, &String)> = unseen.iter().rev().take(reads).copied().collect();
        let mut polled = Polled {
            unread: identified
                .iter()
                .filter(|listed| {
                    if first {
                        !read.contains(listed)
                    } else {
                        seen.contains(&listed.0)
                    }
                })
                .map(|(identity, _)| identity.clone())
                .collect(),
            ..Polled::default()
        };
        let mark = cursor.and_then(|cursor| cursor.newest);
        let latest = now.checked_add(AHEAD).unwrap_or(now);
        for (identity, tag) in read {
            let Some(dated) = self.dated(&mut session, tag).await? else {
                continue;
            };
            let believed = |time: Option<Timestamp>| time.filter(|time| *time <= latest);
            let occurred = believed(dated.created)
                .or_else(|| believed(dated.seen))
                .or_else(|| believed(listed.answered))
                .unwrap_or(now);
            if mark.is_none_or(|mark| occurred >= mark) {
                polled.events.push(self.event(tag, occurred)?);
            } else {
                polled.unread.push(identity.clone());
            }
        }
        polled
            .events
            .sort_by_key(|event| std::cmp::Reverse(event.occurred()));
        if first || unseen.len() <= page {
            polled.newest = polled.events.iter().map(Event::occurred).chain(mark).max();
        }
        Ok(polled)
    }

    fn event(&self, tag: &str, occurred: Timestamp) -> Result<Event, Failure> {
        let mut attributes = versioned(tag.to_owned(), prerelease_version(tag));
        let package = self
            .name
            .split_once('/')
            .map_or(self.name.as_str(), |(_, package)| package);
        attributes.insert(PACKAGE.to_owned(), Attribute::Text(package.to_owned()));
        attributes.insert(
            PACKAGE_TYPE.to_owned(),
            Attribute::Text(CONTAINER.to_owned()),
        );
        let links = if self.reached.docker_hub && self.visibility == Visibility::Public {
            BTreeMap::from([(RELEASE_LINK.to_owned(), docker_hub::page(&self.name, tag))])
        } else {
            BTreeMap::new()
        };
        let draft = Draft {
            identity: self.identity(tag).as_str().to_owned(),
            kind: Kind::PACKAGE_VERSION,
            project: self.project().to_owned(),
            title: format!("{} {tag}", self.project()),
            attributes,
            body: String::new(),
            links,
            occurred,
        };
        Event::normalise(draft)
            .map(|event| event.seen_as(self.visibility).from_origin(self.origin()))
            .map_err(invalid)
    }

    /// What reaching the repository finds wrong: a token service the source
    /// may not trust, a credential the registry refuses, or a config blob
    /// redirected to an origin `blobs` does not name.
    pub(crate) async fn check(&self, http: &dyn DynHttp) -> Vec<Probed> {
        let mut session = self.session(http);
        let request = session.get(&format!("/v2/{}/tags/list?n=1", self.name));
        let answer = match session.send(request).await {
            Ok(answer) => answer,
            Err(failure) => return vec![failure.into()],
        };
        let tag = answered(wire::TAGS, &answer)
            .and_then(|()| wire::tags(self.reached.reading.parse, &answer.body));
        let tag = match tag {
            Ok(tags) => tags.into_iter().next(),
            Err(error) => return vec![Failure::read("repository", &error).into()],
        };
        let Some(tag) = tag else {
            return Vec::new();
        };
        match self.dated(&mut session, &tag).await {
            Ok(Some(Dated {
                redirected: Some(origin),
                ..
            })) => vec![Probed {
                key: "blobs",
                why: format!(
                    "the registry redirects {}'s config blobs to {origin}, which blobs does not name, so its tags take the time they are first seen; add it to blobs where it is the registry's own",
                    self.name
                ),
            }],
            Ok(_) => Vec::new(),
            Err(failure) => vec![failure.into()],
        }
    }
}

/// Why a tag makes no event, as `problem` says.
pub(crate) fn invalid(problem: Problem) -> Failure {
    Failure::read(
        "repository",
        &SourceError::Invalid {
            what: wire::TAGS,
            problem,
        },
    )
}

impl Poll for Image {
    fn api(&self) -> &str {
        &self.reached.registry
    }

    fn bases(&self) -> &[String] {
        &self.reached.bases
    }

    fn remembered(&self) -> usize {
        self.reached.reading.remembered
    }

    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        cursor: Option<&'a Cursor>,
        now: Timestamp,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(async move {
            self.poll(http, cursor, now)
                .await
                .map(Some)
                .map_err(Unpolled::from)
        })
    }

    fn probed<'a>(&'a self, http: &'a dyn DynHttp) -> Boxed<'a, Vec<Probed>> {
        Box::pin(self.check(http))
    }
}
