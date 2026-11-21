//! GitHub's code scanning, Dependabot and secret scanning alerts.

use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{AlertAction, ForgeAlert, Severity, Tool};
use crate::forge::Repository;
use crate::wire::read;

/// The resolution of a secret scanning alert whose secret was revoked.
const REVOKED: &str = "revoked";

/// What raises alerts on GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scanner {
    /// Code scanning.
    CodeScanning,
    /// Dependabot.
    Dependabot,
    /// Secret scanning.
    SecretScanning,
}

impl Scanner {
    /// Every scanner.
    pub(crate) const ALL: [Self; 3] = [Self::CodeScanning, Self::Dependabot, Self::SecretScanning];

    /// The tool its alerts are read as.
    pub(crate) const fn tool(self) -> Tool {
        match self {
            Self::CodeScanning => Tool::CodeScanning,
            Self::Dependabot => Tool::DependencyScanning,
            Self::SecretScanning => Tool::SecretScanning,
        }
    }

    /// The part of a repository's API path its alerts are listed under,
    /// and the name a poll keeps its listing's validators by.
    pub(crate) const fn segment(self) -> &'static str {
        match self {
            Self::CodeScanning => "code-scanning",
            Self::Dependabot => "dependabot",
            Self::SecretScanning => "secret-scanning",
        }
    }

    /// What its alert event is read as.
    pub(crate) const fn hooked_what(self) -> &'static str {
        match self {
            Self::CodeScanning => "GitHub's code scanning alert event",
            Self::Dependabot => "GitHub's Dependabot alert event",
            Self::SecretScanning => "GitHub's secret scanning alert event",
        }
    }

    /// What its listing of alerts is read as.
    pub(crate) const fn listed_what(self) -> &'static str {
        match self {
            Self::CodeScanning => "GitHub's code scanning alerts",
            Self::Dependabot => "GitHub's Dependabot alerts",
            Self::SecretScanning => "GitHub's secret scanning alerts",
        }
    }

    /// The scanner whose alerts GitHub's event `name` is of, where it is
    /// one.
    pub(crate) fn hooked(name: &str) -> Option<Self> {
        match name {
            "code_scanning_alert" => Some(Self::CodeScanning),
            "dependabot_alert" => Some(Self::Dependabot),
            "secret_scanning_alert" => Some(Self::SecretScanning),
            _ => None,
        }
    }
}

/// What an alert's state, as GitHub's listing names it, says of it: its
/// first sighting where it is open, else how it closed and when.
fn listed_action(state: &str, resolution: Option<&str>) -> Option<AlertAction> {
    match (state, resolution) {
        ("open", _) => Some(AlertAction::Created),
        ("fixed", _) | ("resolved", Some(REVOKED)) => Some(AlertAction::Fixed),
        ("dismissed" | "auto_dismissed" | "resolved", _) => Some(AlertAction::Dismissed),
        _ => None,
    }
}

/// What an alert event's action, as GitHub names it for `scanner`, says of
/// the alert: nothing for an action that neither opens nor closes it.
fn hooked_action(scanner: Scanner, action: &str, resolution: Option<&str>) -> Option<AlertAction> {
    match (scanner, action) {
        (_, "created") => Some(AlertAction::Created),
        (_, "reopened" | "reopened_by_user" | "reintroduced" | "auto_reopened") => {
            Some(AlertAction::Reopened)
        }
        (Scanner::CodeScanning | Scanner::Dependabot, "fixed") => Some(AlertAction::Fixed),
        (Scanner::CodeScanning, "closed_by_user")
        | (Scanner::Dependabot, "dismissed" | "auto_dismissed") => Some(AlertAction::Dismissed),
        (Scanner::SecretScanning, "resolved") => listed_action("resolved", resolution),
        _ => None,
    }
}

/// A code scanning alert's rule.
#[derive(Deserialize)]
struct Rule {
    id: Option<String>,
    severity: Option<String>,
    security_severity_level: Option<String>,
    description: Option<String>,
}

/// Where a code scanning result was found.
#[derive(Deserialize)]
struct Location {
    path: Option<String>,
}

/// A code scanning alert's newest result.
#[derive(Deserialize)]
struct Instance {
    location: Option<Location>,
}

/// A Dependabot alert's package.
#[derive(Deserialize)]
struct Package {
    ecosystem: Option<String>,
    name: Option<String>,
}

/// What a Dependabot alert depends on.
#[derive(Deserialize)]
struct Dependency {
    package: Option<Package>,
    manifest_path: Option<String>,
}

/// The advisory a Dependabot alert is of.
#[derive(Deserialize)]
struct Advisory {
    ghsa_id: Option<String>,
    summary: Option<String>,
    severity: Option<String>,
}

/// The repository an owner's listing names an alert of.
#[derive(Deserialize)]
struct Owned {
    full_name: String,
}

/// An alert, as GitHub describes it for any tool: only what herald reads
/// of it, so a secret's value is never parsed.
#[derive(Deserialize)]
struct Alert {
    number: u64,
    state: String,
    html_url: String,
    created_at: Timestamp,
    updated_at: Option<Timestamp>,
    fixed_at: Option<Timestamp>,
    dismissed_at: Option<Timestamp>,
    auto_dismissed_at: Option<Timestamp>,
    resolved_at: Option<Timestamp>,
    resolution: Option<String>,
    rule: Option<Rule>,
    most_recent_instance: Option<Instance>,
    dependency: Option<Dependency>,
    security_advisory: Option<Advisory>,
    secret_type: Option<String>,
    secret_type_display_name: Option<String>,
    repository: Option<Owned>,
}

impl Alert {
    /// The alert `scanner` raised, as it is once it did `action`.
    fn read(self, scanner: Scanner, action: AlertAction) -> ForgeAlert {
        let closed = self
            .fixed_at
            .or(self.dismissed_at)
            .or(self.auto_dismissed_at)
            .or(self.resolved_at);
        let occurred = match action {
            AlertAction::Created => self.created_at,
            AlertAction::Reopened | AlertAction::Confirmed => {
                self.updated_at.unwrap_or(self.created_at)
            }
            AlertAction::Fixed | AlertAction::Dismissed => {
                closed.or(self.updated_at).unwrap_or(self.created_at)
            }
        };
        let (rule, severity, summary) = match scanner {
            Scanner::CodeScanning => {
                let rule = self.rule;
                (
                    rule.as_ref().and_then(|rule| rule.id.clone()),
                    rule.as_ref().and_then(|rule| {
                        rule.security_severity_level
                            .clone()
                            .or_else(|| rule.severity.clone())
                    }),
                    rule.and_then(|rule| rule.description),
                )
            }
            Scanner::Dependabot => {
                let advisory = self.security_advisory;
                (
                    advisory
                        .as_ref()
                        .and_then(|advisory| advisory.ghsa_id.clone()),
                    advisory
                        .as_ref()
                        .and_then(|advisory| advisory.severity.clone()),
                    advisory.and_then(|advisory| advisory.summary),
                )
            }
            Scanner::SecretScanning => (self.secret_type, None, self.secret_type_display_name),
        };
        let (package, ecosystem, manifest) = match self.dependency {
            Some(dependency) => {
                let package = dependency.package;
                (
                    package.as_ref().and_then(|package| package.name.clone()),
                    package.and_then(|package| package.ecosystem),
                    dependency.manifest_path,
                )
            }
            None => (None, None, None),
        };
        let path = manifest.or_else(|| {
            self.most_recent_instance
                .and_then(|instance| instance.location)
                .and_then(|location| location.path)
        });
        ForgeAlert {
            tool: scanner.tool(),
            number: self.number,
            action,
            severity: severity.as_deref().and_then(Severity::named),
            rule,
            summary,
            package,
            ecosystem,
            path,
            resolution: self.resolution,
            url: self.html_url,
            occurred,
            repository: self.repository.map(|repository| repository.full_name),
        }
    }
}

/// A repository, as GitHub's events name it.
#[derive(Deserialize)]
struct Named {
    id: u64,
    name: String,
    full_name: String,
    html_url: Option<String>,
}

/// One of GitHub's alert events.
#[derive(Deserialize)]
struct Hooked {
    action: String,
    alert: Alert,
    repository: Named,
}

/// What `scanner`'s alert event names: the repository's full path, the
/// repository, and the alert where its action opens or closes it.
pub(super) fn delivered(
    scanner: Scanner,
    payload: &[u8],
) -> Result<(String, Repository, Option<ForgeAlert>), SourceError> {
    let hooked: Hooked = read(scanner.hooked_what(), payload)?;
    let action = hooked_action(scanner, &hooked.action, hooked.alert.resolution.as_deref());
    let repository = hooked.repository;
    Ok((
        repository.full_name,
        Repository {
            id: repository.id,
            name: repository.name,
            page: repository.html_url,
        },
        action.map(|action| hooked.alert.read(scanner, action)),
    ))
}

/// The alerts `scanner` raised in GitHub's listing `body`, parsing no more
/// than `limit` bytes, each as its state says.
pub(crate) fn listed(
    scanner: Scanner,
    limit: usize,
    body: &[u8],
) -> Result<Vec<ForgeAlert>, SourceError> {
    let alerts: Vec<Alert> = herald_wire::read(scanner.listed_what(), limit, body)?;
    Ok(alerts
        .into_iter()
        .filter_map(|alert| {
            listed_action(&alert.state, alert.resolution.as_deref())
                .map(|action| alert.read(scanner, action))
        })
        .collect())
}

/// The path, under a repository, of the newest `page` alerts `scanner`
/// raised, the most recently updated first.
pub(super) fn listing_path(scanner: Scanner, page: usize) -> String {
    format!(
        "/{}/alerts?sort=updated&direction=desc&per_page={page}",
        scanner.segment()
    )
}
