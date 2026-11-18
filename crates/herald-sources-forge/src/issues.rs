//! A repository's issues, each announced as the `issue` signal.

use std::collections::BTreeMap;

use herald_core::{
    ACTION, AUTHOR, Attribute, BOT, Cursor, Draft, Event, ISSUE_LINK, Kind, LABELS, NUMBER, SIGNAL,
    STATE_REASON, Signal, Visibility,
};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased};
use herald_wire::SourceError;
use jiff::Timestamp;

use crate::forge::Repository;
pub(crate) use crate::listing::Asked;
use crate::listing::Named;
use crate::options::{IssueAction, IssueChoice};
use crate::runs::Found;
use crate::{Forge, Origin, Token};

mod github;
mod gitlab;

/// An issue, as herald reads it from any forge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeIssue {
    pub(crate) number: u64,
    pub(crate) action: IssueAction,
    pub(crate) title: String,
    pub(crate) author: Option<String>,
    pub(crate) bot: bool,
    pub(crate) state_reason: Option<String>,
    pub(crate) labels: Vec<String>,
    pub(crate) confidential: bool,
    pub(crate) url: String,
    pub(crate) occurred: Timestamp,
}

impl ForgeIssue {
    /// Whether `choice` announces it.
    pub(crate) fn chosen(&self, choice: &IssueChoice) -> bool {
        choice.takes(
            (self.action, self.confidential),
            (self.author.as_deref(), &self.labels),
        )
    }

    /// The event it makes as an issue of `repository`, read as `what`,
    /// named on the host of the repository's page, or else of its own; seen
    /// as private where it is confidential.
    pub(crate) fn event(
        self,
        what: &'static str,
        repository: &Repository,
    ) -> Result<Event, SourceError> {
        let host = crate::wire::host_named(what, repository, &self.url)?;
        let acted = match self.action {
            IssueAction::Opened => self.action.name().to_owned(),
            action => format!("{}/{}", action.name(), self.occurred.as_second()),
        };
        let identity = format!("{host}/{}/issue/{}/{acted}", repository.id, self.number);
        let title = format!(
            "Issue #{} {}: {}",
            self.number,
            self.action.name(),
            self.title
        );
        let mut attributes = BTreeMap::from([
            (
                SIGNAL.to_owned(),
                Attribute::Text(Signal::Issue.name().to_owned()),
            ),
            (
                ACTION.to_owned(),
                Attribute::Text(self.action.name().to_owned()),
            ),
            (NUMBER.to_owned(), Attribute::Number(self.number.into())),
            (BOT.to_owned(), Attribute::Flag(self.bot)),
        ]);
        let labels = (!self.labels.is_empty()).then(|| self.labels.join(", "));
        for (name, value) in [
            (AUTHOR, self.author),
            (STATE_REASON, self.state_reason),
            (LABELS, labels),
        ] {
            if let Some(value) = value {
                attributes.insert(name.to_owned(), Attribute::Text(value));
            }
        }
        let draft = Draft {
            identity,
            kind: Kind::ISSUE,
            project: repository.name.clone(),
            title,
            body: String::new(),
            links: BTreeMap::from([(ISSUE_LINK.to_owned(), self.url)]),
            attributes,
            occurred: self.occurred,
        };
        let confidential = self.confidential;
        let event =
            Event::normalise(draft).map_err(|problem| SourceError::Invalid { what, problem })?;
        Ok(if confidential {
            event.seen_as(Visibility::Private)
        } else {
            event
        })
    }
}

/// What an issue's webhook delivery is read as on each forge.
pub(crate) const HOOKED: Named = Named {
    github: "GitHub's issue event",
    gitlab: "GitLab's issue event",
    forgejo: "Forgejo's issue event",
};

/// What a listing of issues is read as on each forge.
const LISTED: Named = Named {
    github: "GitHub's issues",
    gitlab: "GitLab's issues",
    forgejo: "Forgejo's issues",
};

/// What an issue's webhook delivery is read as on `forge`.
pub(crate) const fn hooked_what(forge: Forge) -> &'static str {
    HOOKED.on(forge)
}

/// Whether the event `name` on `forge` is of an issue.
pub(crate) fn hooked(forge: Forge, name: &str) -> bool {
    match forge {
        Forge::GitLab => name == gitlab::HOOK || name == gitlab::CONFIDENTIAL_HOOK,
        Forge::GitHub | Forge::Forgejo => name == github::HOOK,
    }
}

/// What an issue's webhook delivery names: the repository's full path,
/// and the issue's event where `choice` announces it.
#[derive(Debug)]
pub(crate) struct Delivered {
    pub(crate) path: String,
    pub(crate) event: Option<Event>,
}

/// What the issue delivery `payload` on `forge`, of the event named `name`,
/// names, its event where `choice` announces it.
///
/// # Errors
///
/// When the payload is not the issue event it is named, or its issue makes
/// no event.
pub(crate) fn delivered(
    forge: Forge,
    (name, payload): (&str, &[u8]),
    choice: &IssueChoice,
) -> Result<Delivered, SourceError> {
    let what = hooked_what(forge);
    let (path, repository, issue) = match forge {
        Forge::GitLab => gitlab::delivered(name, payload)?,
        Forge::GitHub | Forge::Forgejo => github::delivered(what, payload)?,
    };
    let event = issue
        .filter(|issue| issue.chosen(choice))
        .map(|issue| issue.event(what, &repository))
        .transpose()?;
    Ok(Delivered { path, event })
}

/// The name a poll keeps the validators of a repository's issue listing
/// under.
pub(crate) const KEPT: &str = "issue";

/// How a repository's issues are polled.
pub(crate) struct Asking<'a> {
    pub(crate) origin: &'a Origin,
    pub(crate) token: Option<&'a Token>,
    pub(crate) choice: &'a IssueChoice,
}

impl Asking<'_> {
    /// The listing of the issues since `cursor`, asked through `http`
    /// conditional on what `cursor` keeps, with its answer.
    pub(crate) async fn asked(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        cursor: Option<&Cursor>,
    ) -> Result<Asked, Unpolled> {
        let page = self.origin.reading().page;
        let path = match self.origin.forge() {
            Forge::GitHub => github::listing_path(page),
            Forge::GitLab => gitlab::listing_path(page, self.choice.confidential),
            Forge::Forgejo => github::forgejo_listing_path(page),
        };
        Asked::asked(http, (self.origin, self.token), &path, (cursor, KEPT)).await
    }

    /// The issues of `repository` the listing `asked` found.
    pub(crate) fn read(&self, asked: &Asked, repository: &Repository) -> Result<Found, Unpolled> {
        let forge = self.origin.forge();
        let what = LISTED.on(forge);
        let mut found = Found::default();
        let (kept, body) = asked.read(what)?;
        if let Some(body) = body {
            let parse = self.origin.reading().parse;
            let issues = match forge {
                Forge::GitLab => gitlab::listed(parse, body),
                Forge::GitHub | Forge::Forgejo => github::listed(what, parse, body),
            }
            .map_err(Unpolled::unreadable)?;
            for issue in issues.into_iter().filter(|issue| issue.chosen(self.choice)) {
                found.events.push(
                    issue
                        .event(what, repository)
                        .map_err(Unpolled::unreadable)?,
                );
            }
        }
        found.listings.insert(KEPT.to_owned(), kept);
        Ok(found)
    }
}

#[cfg(test)]
mod tests;
