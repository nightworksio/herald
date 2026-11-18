//! The keys that choose a forge source's security alerts.

use herald_core::Signal;
use schemars::JsonSchema;
use serde::Deserialize;

use super::SourceOptions;
use crate::Forge;

/// What raised an alert, in words every forge's alerts are read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    /// Code scanning: GitHub's code scanning, such as `CodeQL`, and
    /// GitLab's SAST.
    CodeScanning,
    /// Dependency scanning: GitHub's Dependabot and GitLab's dependency
    /// scanning.
    DependencyScanning,
    /// Secret scanning: GitHub's secret scanning and GitLab's secret
    /// detection.
    SecretScanning,
    /// GitLab's container and cluster image scanning.
    ContainerScanning,
    /// GitLab's dynamic application security testing.
    Dast,
    /// GitLab's coverage-guided and API fuzzing.
    Fuzzing,
    /// What GitLab reads from any other scanner's report.
    Other,
}

impl Tool {
    /// Every tool.
    pub const ALL: [Self; 7] = [
        Self::CodeScanning,
        Self::DependencyScanning,
        Self::SecretScanning,
        Self::ContainerScanning,
        Self::Dast,
        Self::Fuzzing,
        Self::Other,
    ];
    /// The tools whose alerts GitHub raises.
    pub const GITHUB: [Self; 3] = [
        Self::CodeScanning,
        Self::DependencyScanning,
        Self::SecretScanning,
    ];

    /// Its name, as `alerts.tools` writes it and an alert's `tool`
    /// attribute holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CodeScanning => "code-scanning",
            Self::DependencyScanning => "dependency-scanning",
            Self::SecretScanning => "secret-scanning",
            Self::ContainerScanning => "container-scanning",
            Self::Dast => "dast",
            Self::Fuzzing => "fuzzing",
            Self::Other => "other",
        }
    }

    /// The tools whose alerts `forge` raises.
    #[must_use]
    pub const fn on(forge: Forge) -> &'static [Self] {
        match forge {
            Forge::GitHub => &Self::GITHUB,
            Forge::GitLab => &Self::ALL,
            Forge::Forgejo => &[],
        }
    }
}

/// What an alert did that is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AlertAction {
    /// It was raised.
    Created,
    /// It was opened again after it closed.
    Reopened,
    /// Someone confirmed it is real: GitLab's confirmed vulnerability.
    Confirmed,
    /// What it found was fixed: a secret scanning alert's secret revoked,
    /// a GitLab vulnerability resolved.
    Fixed,
    /// It was dismissed, or a secret scanning alert resolved otherwise
    /// than by revoking its secret.
    Dismissed,
}

impl AlertAction {
    /// Its name, as `alerts.actions` writes it and an alert's `action`
    /// attribute holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Reopened => "reopened",
            Self::Confirmed => "confirmed",
            Self::Fixed => "fixed",
            Self::Dismissed => "dismissed",
        }
    }
}

/// How severe an alert is, in lowercase on every forge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Critical.
    Critical,
    /// High.
    High,
    /// Medium, GitHub's `moderate` for a Dependabot alert.
    Medium,
    /// Low.
    Low,
    /// GitLab's informational.
    Info,
    /// GitLab's unknown.
    Unknown,
    /// A code scanning rule's error, where its alert has no security
    /// severity.
    Error,
    /// A code scanning rule's warning.
    Warning,
    /// A code scanning rule's note.
    Note,
}

impl Severity {
    /// Every severity.
    pub const ALL: [Self; 9] = [
        Self::Critical,
        Self::High,
        Self::Medium,
        Self::Low,
        Self::Info,
        Self::Unknown,
        Self::Error,
        Self::Warning,
        Self::Note,
    ];

    /// Its name, as `alerts.severities` writes it and an alert's
    /// `severity` attribute holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Info => "info",
            Self::Unknown => "unknown",
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
        }
    }

    /// The severity a forge names `name`, in any case, where it names one.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        let name = if name.eq_ignore_ascii_case("moderate") {
            Self::Medium.name()
        } else {
            name
        };
        Self::ALL
            .into_iter()
            .find(|severity| severity.name().eq_ignore_ascii_case(name))
    }
}

/// Which alerts a source announces, where its signals take "alert".
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Alerts {
    /// What raised the alerts announced; every tool the forge raises
    /// alerts with where absent.
    pub tools: Option<Vec<Tool>>,
    /// What an alert did that is announced: `created`, `reopened`,
    /// `confirmed` (GitLab), `fixed`, `dismissed`; `["created",
    /// "reopened"]` where absent.
    pub actions: Option<Vec<AlertAction>>,
    /// How severe an alert is for it to be announced; every alert, those
    /// with no severity too, where empty.
    #[serde(default)]
    pub severities: Vec<Severity>,
}

/// The actions announced where `alerts.actions` is absent.
pub const ALERT_ACTIONS: [AlertAction; 2] = [AlertAction::Created, AlertAction::Reopened];

/// Which alerts a source announces, read from its `alerts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AlertChoice {
    pub(crate) tools: Vec<Tool>,
    pub(crate) actions: Vec<AlertAction>,
    pub(crate) severities: Vec<Severity>,
}

impl AlertChoice {
    /// Whether it reads the alerts `tool` raises.
    pub(crate) fn reads(&self, tool: Tool) -> bool {
        self.tools.contains(&tool)
    }

    /// Whether it announces an alert `tool` raised, of `severity`, that
    /// did `action`.
    pub(crate) fn takes(
        &self,
        tool: Tool,
        action: AlertAction,
        severity: Option<Severity>,
    ) -> bool {
        self.reads(tool)
            && self.actions.contains(&action)
            && (self.severities.is_empty()
                || severity.is_some_and(|severity| self.severities.contains(&severity)))
    }
}

impl SourceOptions {
    /// Which alerts the source on `forge` announces.
    pub(crate) fn alert_choice(&self, forge: Forge) -> AlertChoice {
        let alerts = self.alerts.clone().unwrap_or_default();
        AlertChoice {
            tools: alerts.tools.unwrap_or_else(|| Tool::on(forge).to_vec()),
            actions: alerts.actions.unwrap_or_else(|| ALERT_ACTIONS.to_vec()),
            severities: alerts.severities,
        }
    }

    /// Whether `alerts` is sound on `forge`, and given only where some
    /// repository the source takes announces alerts, which Forgejo raises
    /// none of.
    pub(super) fn alerts_check(&self, forge: Forge) -> Result<(), String> {
        let alerted = self.may_take(Signal::Alert);
        if alerted && forge == Forge::Forgejo {
            return Err(
                "Forgejo raises no security alerts; remove \"alert\" from signals".to_owned(),
            );
        }
        let Some(alerts) = &self.alerts else {
            return Ok(());
        };
        if !alerted {
            return Err(
                "alerts chooses security alerts, which no repository the source takes announces; add \"alert\" to signals, or remove alerts".to_owned(),
            );
        }
        let lists = [
            ("tools", alerts.tools.as_ref().is_some_and(Vec::is_empty)),
            (
                "actions",
                alerts.actions.as_ref().is_some_and(Vec::is_empty),
            ),
        ];
        if let Some((key, _)) = lists.into_iter().find(|(_, empty)| *empty) {
            return Err(format!(
                "alerts.{key} names nothing, so no alert would be announced; name one, or remove alerts.{key}"
            ));
        }
        let raised = Tool::on(forge);
        if let Some(tool) = alerts
            .tools
            .iter()
            .flatten()
            .find(|tool| !raised.contains(tool))
        {
            return Err(format!(
                "alerts.tools \"{}\": {} raises no such alerts; its tools are {}",
                tool.name(),
                forge.kind(),
                raised
                    .iter()
                    .map(|tool| format!("\"{}\"", tool.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if forge != Forge::GitLab
            && alerts
                .actions
                .iter()
                .flatten()
                .any(|action| *action == AlertAction::Confirmed)
        {
            return Err(format!(
                "alerts.actions \"confirmed\" is GitLab's, and {} confirms no alert; remove it",
                forge.kind()
            ));
        }
        Ok(())
    }
}
