//! A repository's pull requests, each announced as the `pull-request`
//! signal: GitHub's and Forgejo's pull requests, and GitLab's merge
//! requests.

use std::collections::BTreeMap;

use herald_core::{
    ACTION, AUTHOR, Attribute, BASE, BOT, Cursor, DRAFT, Draft, Event, HEAD, Kind, LABELS,
    MERGED_BY, NUMBER, PULL_LINK, SIGNAL, Signal, Validators,
};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Response};
use herald_wire::{SourceError, answered};
use jiff::Timestamp;

use crate::forge::{Repository, validators};
use crate::options::{PullAction, PullChoice};
use crate::runs::Found;
use crate::{Forge, Origin, Token};

mod github;
mod gitlab;

/// What a pull request's title says it did.
const fn did(action: PullAction) -> &'static str {
    match action {
        PullAction::Opened => "opened",
        PullAction::ReadyForReview => "ready for review",
        PullAction::Reopened => "reopened",
        PullAction::Merged => "merged",
        PullAction::Closed => "closed",
    }
}

/// How a pull request's title names it on `forge`.
const fn named(forge: Forge) -> (&'static str, char) {
    match forge {
        Forge::GitLab => ("Merge request", '!'),
        Forge::GitHub | Forge::Forgejo => ("Pull request", '#'),
    }
}

/// A pull request, as herald reads it from any forge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgePull {
    pub(crate) number: u64,
    pub(crate) action: PullAction,
    pub(crate) title: String,
    pub(crate) author: Option<String>,
    pub(crate) bot: bool,
    pub(crate) base: Option<String>,
    pub(crate) head: Option<String>,
    pub(crate) draft: bool,
    pub(crate) merged_by: Option<String>,
    pub(crate) labels: Vec<String>,
    pub(crate) url: String,
    pub(crate) occurred: Timestamp,
}

impl ForgePull {
    /// Whether `choice` announces it.
    pub(crate) fn chosen(&self, choice: &PullChoice) -> bool {
        choice.takes(
            (self.action, self.draft),
            (self.author.as_deref(), &self.labels),
        )
    }

    /// The last part of its identity: `opened` for its opening, which a
    /// webhook and a poll share, and else what it did and when.
    fn acted(&self) -> String {
        match self.action {
            PullAction::Opened => self.action.name().to_owned(),
            action => format!("{}/{}", action.name(), self.occurred.as_second()),
        }
    }

    /// The event it makes as a pull request of `repository` on `forge`,
    /// named on the host of the repository's page, or else of its own.
    pub(crate) fn event(
        self,
        (forge, what): (Forge, &'static str),
        repository: &Repository,
    ) -> Result<Event, SourceError> {
        let host = crate::wire::host_named(what, repository, &self.url)?;
        let identity = format!(
            "{host}/{}/pull/{}/{}",
            repository.id,
            self.number,
            self.acted()
        );
        let (noun, sign) = named(forge);
        let title = format!(
            "{noun} {sign}{} {}: {}",
            self.number,
            did(self.action),
            self.title
        );
        let text = |value: &str| Attribute::Text(value.to_owned());
        let mut attributes = BTreeMap::from([
            (SIGNAL.to_owned(), text(Signal::PullRequest.name())),
            (ACTION.to_owned(), text(self.action.name())),
            (NUMBER.to_owned(), Attribute::Number(self.number.into())),
            (BOT.to_owned(), Attribute::Flag(self.bot)),
            (DRAFT.to_owned(), Attribute::Flag(self.draft)),
        ]);
        let labels = (!self.labels.is_empty()).then(|| self.labels.join(", "));
        let named = [
            (AUTHOR, self.author),
            (BASE, self.base),
            (HEAD, self.head),
            (MERGED_BY, self.merged_by),
            (LABELS, labels),
        ];
        for (name, value) in named {
            if let Some(value) = value {
                attributes.insert(name.to_owned(), Attribute::Text(value));
            }
        }
        let draft = Draft {
            identity,
            kind: Kind::PULL_REQUEST,
            project: repository.name.clone(),
            title,
            body: String::new(),
            links: BTreeMap::from([(PULL_LINK.to_owned(), self.url)]),
            attributes,
            occurred: self.occurred,
        };
        Event::normalise(draft).map_err(|problem| SourceError::Invalid { what, problem })
    }
}

/// What a pull request's webhook delivery is read as on `forge`.
pub(crate) const fn hooked_what(forge: Forge) -> &'static str {
    match forge {
        Forge::GitHub => github::HOOKED,
        Forge::GitLab => gitlab::HOOKED,
        Forge::Forgejo => github::FORGEJO_HOOKED,
    }
}

/// What a listing of pull requests is read as on `forge`.
const fn listed_what(forge: Forge) -> &'static str {
    match forge {
        Forge::GitHub => github::LISTED,
        Forge::GitLab => gitlab::LISTED,
        Forge::Forgejo => github::FORGEJO_LISTED,
    }
}

/// Whether the event `name` on `forge` is of a pull request.
pub(crate) fn hooked(forge: Forge, name: &str) -> bool {
    match forge {
        Forge::GitLab => name == gitlab::HOOK,
        Forge::GitHub | Forge::Forgejo => name == github::HOOK,
    }
}

/// What a pull request's webhook delivery names: the repository's full
/// path, and the pull request's event where `choice` announces it.
#[derive(Debug)]
pub(crate) struct Delivered {
    pub(crate) path: String,
    pub(crate) event: Option<Event>,
}

/// What the pull request delivery `payload` on `forge` names, its event
/// where `choice` announces it.
///
/// # Errors
///
/// When the payload is not the pull request event it is named, or its pull
/// request makes no event.
pub(crate) fn delivered(
    forge: Forge,
    payload: &[u8],
    choice: &PullChoice,
) -> Result<Delivered, SourceError> {
    let what = hooked_what(forge);
    let (path, repository, pull) = match forge {
        Forge::GitLab => gitlab::delivered(payload)?,
        Forge::GitHub | Forge::Forgejo => github::delivered(what, payload)?,
    };
    let event = pull
        .filter(|pull| pull.chosen(choice))
        .map(|pull| pull.event((forge, what), &repository))
        .transpose()?;
    Ok(Delivered { path, event })
}

/// The name a poll keeps the validators of a repository's pull request
/// listing under.
pub(crate) const KEPT: &str = "pull-request";

/// How a repository's pull requests are polled.
pub(crate) struct Asking<'a> {
    pub(crate) origin: &'a Origin,
    pub(crate) token: Option<&'a Token>,
    pub(crate) choice: &'a PullChoice,
}

impl Asking<'_> {
    /// The listing of the pull requests since `cursor`, asked through
    /// `http` conditional on what `cursor` keeps, with its answer.
    pub(crate) async fn asked(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        cursor: Option<&Cursor>,
    ) -> Result<Asked, Unpolled> {
        let page = self.origin.reading().page;
        let path = match self.origin.forge() {
            Forge::GitHub => github::listing_path(page),
            Forge::GitLab => gitlab::listing_path(page),
            Forge::Forgejo => github::forgejo_listing_path(page),
        };
        let kept = cursor.and_then(|cursor| cursor.listings.get(KEPT)).cloned();
        let request = self.origin.get(&path, self.token).conditional(
            kept.as_ref().and_then(|kept| kept.etag.as_deref()),
            kept.as_ref().and_then(|kept| kept.last_modified.as_deref()),
        );
        let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
        Ok(Asked { kept, answer })
    }

    /// The pull requests of `repository` the listing `asked` found.
    pub(crate) fn read(&self, asked: &Asked, repository: &Repository) -> Result<Found, Unpolled> {
        let forge = self.origin.forge();
        let mut found = Found::default();
        let kept = if Forge::unchanged(&asked.answer) {
            asked.kept.clone().unwrap_or_default()
        } else {
            let what = listed_what(forge);
            answered(what, &asked.answer).map_err(Unpolled::unreadable)?;
            let parse = self.origin.reading().parse;
            let pulls = match forge {
                Forge::GitLab => gitlab::listed(parse, &asked.answer.body),
                Forge::GitHub | Forge::Forgejo => github::listed(what, parse, &asked.answer.body),
            }
            .map_err(Unpolled::unreadable)?;
            for pull in pulls.into_iter().filter(|pull| pull.chosen(self.choice)) {
                let event = pull
                    .event((forge, what), repository)
                    .map_err(Unpolled::unreadable)?;
                found.events.push(event);
            }
            validators(&asked.answer)
        };
        found.listings.insert(KEPT.to_owned(), kept);
        Ok(found)
    }
}

/// A poll's listing of pull requests: the validators it was asked under,
/// and its answer.
#[derive(Debug)]
pub(crate) struct Asked {
    kept: Option<Validators>,
    answer: Response,
}

impl Asked {
    /// Whether the listing changed since it was last read.
    pub(crate) fn changed(&self) -> bool {
        !Forge::unchanged(&self.answer)
    }
}

#[cfg(test)]
mod tests;
