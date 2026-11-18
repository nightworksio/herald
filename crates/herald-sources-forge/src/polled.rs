//! One repository polled for each signal its source announces of it: its
//! releases, its tags, its packages' versions, or several, each asked with
//! its own validators (ADR-0031, ADR-0035).

use herald_core::{Address, Cursor, Event, Identity, Polled, Signal};
use herald_kinds::{Poll, Unpolled};
use herald_ports::{Boxed, DynHttp, Erased, Http as _, Response};

use crate::forge::{Repository, validators};
use crate::kind::{originated, projected};
use crate::options::RunChoice;
use crate::packages::{Asking, PackageList, Packaging, merged, packaged_of};
use crate::runs;
use crate::tags::{Listed, Pointed};
use crate::wire::{ForgeTag, tag_event};
use crate::{Forge, Origin, Signals, Token};

/// A repository's releases, tags and package versions, as its source's
/// signals ask, polled with the token the config names, and announced as
/// of the project a template named, where one did; who may see it, where
/// its owner's listing said.
#[derive(Debug)]
pub(crate) struct Releases {
    pub(crate) origin: Origin,
    pub(crate) token: Option<Token>,
    pub(crate) project: Option<Result<String, String>>,
    pub(crate) signals: Signals,
    pub(crate) backfill: usize,
    pub(crate) held: Option<jiff::SignedDuration>,
    pub(crate) watched: usize,
    pub(crate) packaging: Packaging,
    pub(crate) list: PackageList,
    pub(crate) visibility: Option<herald_core::Visibility>,
    pub(crate) runs: RunChoice,
}

/// What the tags answer of a poll found: the events of tags read in full,
/// and the identities of those only remembered.
struct Tagged {
    events: Vec<Event>,
    passed: Vec<Identity>,
}

/// Whether `answer`, where one was asked, says something changed.
fn changed(answer: Option<&Response>) -> bool {
    answer.is_some_and(|answer| !Forge::unchanged(answer))
}

impl Releases {
    async fn ask(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        request: herald_ports::Request,
    ) -> Result<Response, Unpolled> {
        http.send(request).await.map_err(Unpolled::unreachable)
    }

    /// The tag `listed`, read in full through `http` where its listing
    /// leaves something out.
    async fn read(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        listed: Listed,
    ) -> Result<ForgeTag, Unpolled> {
        let (forge, token) = (self.origin.forge(), self.token.as_ref());
        let read = match listed {
            Listed::Read(tag) => return Ok(tag),
            Listed::Referenced(name) => {
                let reference = self
                    .ask(http, self.origin.reference_request(&name, token))
                    .await?;
                match forge.pointed(&reference).map_err(Unpolled::unreadable)? {
                    Pointed::Tag(sha) => {
                        let object = self
                            .ask(http, self.origin.tag_object_request(&sha, token))
                            .await?;
                        forge
                            .tag_object(&object, None)
                            .map(|tag| ForgeTag { name, ..tag })
                    }
                    Pointed::Commit(sha) => {
                        let commit = self
                            .ask(http, self.origin.commit_request(&sha, token))
                            .await?;
                        forge.committed(&commit, &name)
                    }
                }
            }
            Listed::Annotated {
                name,
                object,
                message,
            } => {
                let object = self
                    .ask(http, self.origin.tag_object_request(&object, token))
                    .await?;
                forge
                    .tag_object(&object, message)
                    .map(|tag| ForgeTag { name, ..tag })
            }
        };
        read.map_err(Unpolled::unreadable)
    }

    /// The tags in `answer` of `repository` that no release in `released`
    /// shares an identity with and `cursor` has not seen: each read in full
    /// where the source has a cursor, and with none only the newest
    /// `backfill`, the rest remembered without being read.
    async fn tagged(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (repository, answer): (&Repository, &Response),
        (released, cursor): (&[Event], Option<&Cursor>),
    ) -> Result<Tagged, Unpolled> {
        let forge = self.origin.forge();
        let page = repository.page.as_deref().ok_or_else(|| {
            Unpolled::Unreadable(format!(
                "{} names no web page of the repository",
                forge.api().repository
            ))
        })?;
        let host = Address::parse(page)
            .ok_or_else(|| {
                Unpolled::Unreadable(format!("the repository's page {page} names no host"))
            })?
            .host()
            .to_owned();
        let listed = forge
            .tags(answer, self.origin.reading().parse)
            .map_err(Unpolled::unreadable)?;
        let mut tagged = Tagged {
            events: Vec::new(),
            passed: Vec::new(),
        };
        for entry in listed {
            let identity = crate::wire::identity(&host, repository.id, entry.name());
            let known = released.iter().any(|event| event.identity() == &identity)
                || cursor.is_some_and(|cursor| cursor.seen.contains(&identity));
            if known {
                continue;
            }
            if cursor.is_none() && tagged.events.len() >= self.backfill {
                tagged.passed.push(identity);
                continue;
            }
            let url = forge.tag_page(page, entry.name());
            let tag = self.read(http, entry).await?;
            let event = tag_event(
                forge.api().tags,
                repository,
                (tag, url),
                self.signals.prerelease,
            )
            .map_err(Unpolled::unreadable)?;
            tagged.events.push(event);
        }
        Ok(tagged)
    }
}

/// The event of the tag `tag` of the repository `origin` names, read in
/// full through `http` with `token`, its pre-release told by `rule`: what a
/// CI run for a tag announces (ADR-0032).
///
/// # Errors
///
/// Where the forge cannot be reached, or its answers cannot be read.
pub async fn tag_event_of(
    origin: &Origin,
    http: &dyn DynHttp,
    (tag, token): (&str, Option<&Token>),
    rule: herald_core::PrereleaseRule,
) -> Result<Event, Unpolled> {
    let releases = Releases {
        origin: origin.clone(),
        token: token.cloned(),
        project: None,
        signals: Signals {
            taken: [Signal::Tag].into(),
            prerelease: rule,
        },
        backfill: 0,
        held: None,
        watched: 0,
        packaging: Packaging::of(origin.forge(), &crate::SourceOptions::default()),
        list: PackageList::Own,
        visibility: None,
        runs: RunChoice::default(),
    };
    let http = Erased(http);
    let forge = origin.forge();
    let described = releases
        .ask(&http, origin.repository_request(token))
        .await?;
    let repository = forge.repository(&described).map_err(Unpolled::unreadable)?;
    let listed = match forge {
        Forge::GitHub => Listed::Referenced(tag.to_owned()),
        Forge::GitLab | Forge::Forgejo => {
            let answer = releases.ask(&http, origin.tag_request(tag, token)).await?;
            forge.tag(tag, &answer).map_err(Unpolled::unreadable)?
        }
    };
    let page = repository.page.as_deref().ok_or_else(|| {
        Unpolled::Unreadable(format!(
            "{} names no web page of the repository",
            forge.api().repository
        ))
    })?;
    let origin = origin.described(&described).map_err(Unpolled::unreadable)?;
    let url = forge.tag_page(page, tag);
    let read = releases.read(&http, listed).await?;
    tag_event(forge.api().tags, &repository, (read, url), rule)
        .map(|event| event.from_origin(origin))
        .map_err(Unpolled::unreadable)
}

impl Releases {
    /// How its repository's runs are asked, where its signals take them.
    fn run_asking(&self) -> Option<runs::Asking<'_>> {
        self.signals.takes(Signal::Run).then_some(runs::Asking {
            origin: &self.origin,
            token: self.token.as_ref(),
            choice: &self.runs,
        })
    }
}

impl Poll for Releases {
    fn api(&self) -> &str {
        self.origin.api()
    }

    fn remembered(&self) -> usize {
        self.origin.reading().remembered
    }

    fn tag_window(&self) -> Option<jiff::SignedDuration> {
        self.held
    }

    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        cursor: Option<&'a Cursor>,
        _: jiff::Timestamp,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(async move {
            let http = Erased(http);
            let token = self.token.as_ref();
            let releases = if self.signals.takes(Signal::Release) {
                Some(
                    self.ask(&http, self.origin.releases_request(cursor, token))
                        .await?,
                )
            } else {
                None
            };
            let kept = cursor.and_then(|cursor| cursor.validators(Signal::Tag));
            let tags = if self.signals.takes(Signal::Tag) {
                Some(
                    self.ask(&http, self.origin.tags_request(kept, token))
                        .await?,
                )
            } else {
                None
            };
            let runs = self.run_asking();
            let ran = match &runs {
                Some(runs) => Some(runs.asked(&http, cursor).await?),
                None => None,
            };
            let asks_packages = self.signals.takes(Signal::Package);
            let asked = changed(releases.as_ref())
                || changed(tags.as_ref())
                || ran.as_ref().is_some_and(runs::Asked::changed);
            if !asked && (!asks_packages || matches!(self.list, PackageList::Kept)) {
                return Ok(None);
            }
            let described = self
                .ask(&http, self.origin.repository_request(token))
                .await?;
            let forge = self.origin.forge();
            let repository = forge.repository(&described).map_err(Unpolled::unreadable)?;
            let origin = self
                .origin
                .described(&described)
                .map_err(Unpolled::unreadable)?;
            let packaged = if asks_packages {
                let asking = Asking {
                    origin: &self.origin,
                    token,
                    packaging: &self.packaging,
                    rule: self.signals.prerelease,
                    list: &self.list,
                    repository: self.visibility,
                };
                Some(packaged_of(&http, &asking, (&repository, &described), cursor).await?)
            } else {
                None
            };
            if !asked && !packaged.as_ref().is_some_and(|packaged| packaged.changed) {
                return Ok(None);
            }
            let mut polled = match &releases {
                Some(answer) if changed(Some(answer)) => forge
                    .releases(
                        &repository,
                        answer,
                        (self.origin.reading().parse, self.signals.prerelease),
                    )
                    .map_err(Unpolled::unreadable)?,
                Some(_) => Polled {
                    etag: cursor.and_then(|cursor| cursor.etag.clone()),
                    last_modified: cursor.and_then(|cursor| cursor.last_modified.clone()),
                    ..Polled::default()
                },
                None => Polled::default(),
            };
            if let Some(answer) = &tags {
                let validators = if changed(Some(answer)) {
                    let tagged = self
                        .tagged(&http, (&repository, answer), (&polled.events, cursor))
                        .await?;
                    polled.events.extend(tagged.events);
                    polled.passed = tagged.passed;
                    validators(answer)
                } else {
                    kept.cloned().unwrap_or_default()
                };
                polled.signals.insert(Signal::Tag, validators);
            }
            let polled = match packaged {
                Some(packaged) => merged(Some(polled), packaged, cursor).unwrap_or_default(),
                None => polled,
            };
            let mut polled = match (runs, ran) {
                (Some(runs), Some(ran)) => runs::merged(polled, runs.read(&ran, &repository)?),
                _ => polled,
            };
            polled
                .events
                .sort_by_key(|event| std::cmp::Reverse(event.occurred()));
            polled.watched = self.watched;
            projected(originated(polled, &origin), self.project.clone()).map(Some)
        })
    }
}

#[cfg(test)]
mod tests;
