//! A repository's security alerts, each announced as the `alert` signal:
//! GitHub's code scanning, Dependabot and secret scanning alerts, and
//! GitLab Ultimate's vulnerabilities. An
//! alert's event carries what the alert is, from an allowlist, and never
//! what it found, such as a secret's value.

use std::collections::BTreeMap;

use herald_core::{
    ACTION, ALERT_LINK, Attribute, Cursor, Draft, ECOSYSTEM, Event, Kind, NUMBER, PACKAGE, PATH,
    RESOLUTION, RULE, SEVERITY, SIGNAL, Signal, TOOL, Validators,
};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Request, Response};
use herald_wire::{SourceError, answered};
use jiff::Timestamp;

use crate::forge::{Repository, validators};
use crate::options::{AlertAction, AlertChoice, Severity, Tool};
use crate::runs::Found;
use crate::{Forge, Origin, Token};

mod github;
mod gitlab;

pub(crate) use github::Scanner;

/// How an alert's title names what raised it.
const fn label(tool: Tool) -> &'static str {
    match tool {
        Tool::CodeScanning => "Code scanning",
        Tool::DependencyScanning => "Dependency scanning",
        Tool::SecretScanning => "Secret scanning",
        Tool::ContainerScanning => "Container scanning",
        Tool::Dast => "DAST",
        Tool::Fuzzing => "Fuzzing",
        Tool::Other => "Security",
    }
}

/// What an alert's title says it did.
const fn did(action: AlertAction) -> &'static str {
    match action {
        AlertAction::Created => "opened",
        AlertAction::Reopened => "reopened",
        AlertAction::Confirmed => "confirmed",
        AlertAction::Fixed => "fixed",
        AlertAction::Dismissed => "dismissed",
    }
}

/// An alert, as herald reads it from any tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeAlert {
    pub(crate) tool: Tool,
    pub(crate) number: u64,
    pub(crate) action: AlertAction,
    pub(crate) severity: Option<Severity>,
    pub(crate) rule: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) package: Option<String>,
    pub(crate) ecosystem: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) resolution: Option<String>,
    pub(crate) url: String,
    pub(crate) occurred: Timestamp,
}

impl ForgeAlert {
    /// Whether `choice` announces it.
    pub(crate) fn chosen(&self, choice: &AlertChoice) -> bool {
        choice.takes(self.tool, self.action, self.severity)
    }

    /// The last part of its identity: `created` for its first sighting,
    /// which a webhook and a poll share, and else what it did and when.
    fn acted(&self) -> String {
        match self.action {
            AlertAction::Created => self.action.name().to_owned(),
            action => format!("{}/{}", action.name(), self.occurred.as_second()),
        }
    }

    /// The event it makes as an alert of `repository`, named on the host of
    /// the repository's page, or else of its own.
    pub(crate) fn event(
        self,
        what: &'static str,
        repository: &Repository,
    ) -> Result<Event, SourceError> {
        let host = crate::wire::host_named(what, repository, &self.url)?;
        let identity = format!(
            "{host}/{}/alert/{}/{}/{}",
            repository.id,
            self.tool.name(),
            self.number,
            self.acted()
        );
        let about = self
            .summary
            .as_deref()
            .or(self.rule.as_deref())
            .map_or_else(String::new, |about| format!(": {about}"));
        let title = format!(
            "{} alert #{} {}{about}",
            label(self.tool),
            self.number,
            did(self.action)
        );
        let text = |value: &str| Attribute::Text(value.to_owned());
        let mut attributes = BTreeMap::from([
            (SIGNAL.to_owned(), text(Signal::Alert.name())),
            (ACTION.to_owned(), text(self.action.name())),
            (TOOL.to_owned(), text(self.tool.name())),
            (NUMBER.to_owned(), Attribute::Number(self.number.into())),
        ]);
        let named = [
            (
                SEVERITY,
                self.severity.map(|severity| severity.name().to_owned()),
            ),
            (RULE, self.rule),
            (PACKAGE, self.package),
            (ECOSYSTEM, self.ecosystem),
            (PATH, self.path),
            (RESOLUTION, self.resolution),
        ];
        for (name, value) in named {
            if let Some(value) = value {
                attributes.insert(name.to_owned(), Attribute::Text(value));
            }
        }
        let draft = Draft {
            identity,
            kind: Kind::ALERT,
            project: repository.name.clone(),
            title,
            body: String::new(),
            links: BTreeMap::from([(ALERT_LINK.to_owned(), self.url)]),
            attributes,
            occurred: self.occurred,
        };
        Event::normalise(draft).map_err(|problem| SourceError::Invalid { what, problem })
    }
}

/// What a webhook delivery is, where it is of an alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// One of GitHub's alert events, of its scanner.
    GitHub(Scanner),
    /// GitLab's vulnerability event.
    GitLab,
}

impl Delivery {
    /// What the event named `name` on `forge` is, where it is of an alert.
    pub(crate) fn named(forge: Forge, name: &str) -> Option<Self> {
        match forge {
            Forge::GitHub => Scanner::hooked(name).map(Self::GitHub),
            Forge::GitLab => (name == gitlab::HOOK).then_some(Self::GitLab),
            Forge::Forgejo => None,
        }
    }

    /// What it is read as.
    pub(crate) const fn what(self) -> &'static str {
        match self {
            Self::GitHub(scanner) => scanner.hooked_what(),
            Self::GitLab => gitlab::HOOKED,
        }
    }
}

/// What an alert's webhook delivery names: the repository's full path,
/// the repository, and the alert's event where the source's choice
/// announces it.
pub(crate) struct Delivered {
    pub(crate) path: String,
    pub(crate) repository: Repository,
    pub(crate) event: Option<Event>,
}

/// What the alert delivery `delivery`, `payload`, names on an instance of
/// the API `api`, its event where `choice` announces it.
///
/// # Errors
///
/// When the payload is not the alert event it is named, or its alert makes
/// no event.
pub(crate) fn delivered(
    (delivery, api): (Delivery, &str),
    payload: &[u8],
    choice: &AlertChoice,
) -> Result<Delivered, SourceError> {
    let (path, repository, alert) = match delivery {
        Delivery::GitHub(scanner) => github::delivered(scanner, payload)?,
        Delivery::GitLab => gitlab::delivered(&crate::gitlab::web(api), payload)?,
    };
    let event = alert
        .filter(|alert| alert.chosen(choice))
        .map(|alert| alert.event(delivery.what(), &repository))
        .transpose()?;
    Ok(Delivered {
        path,
        repository,
        event,
    })
}

/// One listing a poll of alerts reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listed {
    /// One of GitHub's scanners' alerts, kept under their validators.
    GitHub(Scanner),
    /// GitLab's newest detected vulnerabilities, asked through GraphQL,
    /// which answers with no validators.
    GitLab,
}

impl Listed {
    /// What its answer is read as.
    const fn what(self) -> &'static str {
        match self {
            Self::GitHub(scanner) => scanner.listed_what(),
            Self::GitLab => gitlab::LISTED,
        }
    }
}

/// What the name a poll keeps an alert listing's validators under starts
/// with, before its scanner's.
const KEPT: &str = "alert/";

/// The name a poll keeps the validators of `scanner`'s listing under.
fn kept_as(scanner: Scanner) -> String {
    format!("{KEPT}{}", scanner.segment())
}

/// How a repository's alerts are polled.
pub(crate) struct Asking<'a> {
    pub(crate) origin: &'a Origin,
    pub(crate) token: Option<&'a Token>,
    pub(crate) choice: &'a AlertChoice,
}

impl Asking<'_> {
    /// The listings a poll reads, with their requests, each of GitHub's
    /// conditional on what `cursor` keeps.
    fn listings(&self, cursor: Option<&Cursor>) -> Vec<(Listed, Option<Validators>, Request)> {
        let reading = self.origin.reading();
        match self.origin.forge() {
            Forge::GitHub => Scanner::ALL
                .into_iter()
                .filter(|scanner| self.choice.reads(scanner.tool()))
                .map(|scanner| {
                    let kept = cursor
                        .and_then(|cursor| cursor.listings.get(&kept_as(scanner)))
                        .cloned();
                    let request = self
                        .origin
                        .get(&github::listing_path(scanner, reading.page), self.token)
                        .conditional(
                            kept.as_ref().and_then(|kept| kept.etag.as_deref()),
                            kept.as_ref().and_then(|kept| kept.last_modified.as_deref()),
                        );
                    (Listed::GitHub(scanner), kept, request)
                })
                .collect(),
            Forge::GitLab => {
                let endpoint = crate::gitlab::graphql(self.origin.api());
                let request = gitlab::listing(
                    &endpoint,
                    (self.origin.path(), self.token),
                    (reading.page, reading.parse),
                );
                vec![(Listed::GitLab, None, request)]
            }
            Forge::Forgejo => Vec::new(),
        }
    }

    /// Each listing of the alerts since `cursor`, asked through `http`,
    /// with its answer.
    pub(crate) async fn asked(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        cursor: Option<&Cursor>,
    ) -> Result<Asked, Unpolled> {
        let mut answers = Vec::new();
        for (listed, kept, request) in self.listings(cursor) {
            let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
            answers.push((listed, kept, answer));
        }
        Ok(Asked(answers))
    }

    /// The alerts of `repository` in `answer` to `listed`.
    fn listed(&self, listed: Listed, answer: &Response) -> Result<Vec<ForgeAlert>, SourceError> {
        let parse = self.origin.reading().parse;
        answered(listed.what(), answer)?;
        match listed {
            Listed::GitHub(scanner) => github::listed(scanner, parse, &answer.body),
            Listed::GitLab => gitlab::listed(parse, &answer.body),
        }
    }

    /// The alerts of `repository` the listings `asked` found.
    pub(crate) fn read(&self, asked: &Asked, repository: &Repository) -> Result<Found, Unpolled> {
        let mut alerts = Found::default();
        for (listed, kept, answer) in &asked.0 {
            let kept = if Forge::unchanged(answer) {
                kept.clone().unwrap_or_default()
            } else {
                let found = self.listed(*listed, answer).map_err(Unpolled::unreadable)?;
                for alert in found.into_iter().filter(|alert| alert.chosen(self.choice)) {
                    let event = alert
                        .event(listed.what(), repository)
                        .map_err(Unpolled::unreadable)?;
                    alerts.events.push(event);
                }
                validators(answer)
            };
            if let Listed::GitHub(scanner) = listed {
                alerts.listings.insert(kept_as(*scanner), kept);
            }
        }
        Ok(alerts)
    }
}

/// Each listing a poll of alerts asked: what it lists, the validators it
/// was asked under, and its answer.
pub(crate) struct Asked(Vec<(Listed, Option<Validators>, Response)>);

impl Asked {
    /// Whether some listing changed since it was last read.
    pub(crate) fn changed(&self) -> bool {
        self.0
            .iter()
            .any(|(_, _, answer)| !Forge::unchanged(answer))
    }
}

#[cfg(test)]
mod tests;
