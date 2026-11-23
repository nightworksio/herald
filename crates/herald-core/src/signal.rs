use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Kind;
use crate::version::prerelease_version;

/// The attribute naming the signal an event is of.
pub const SIGNAL: &str = "signal";

/// The attribute marking an event as an edit of one the forge published
/// before (ADR-0034).
pub const EDITED: &str = "edited";

/// The attribute marking an event as the forge's deletion of one it
/// published before, which a route withdraws (ADR-0048).
pub const WITHDRAWN: &str = "withdrawn";

/// What a source announces of a repository.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Signal {
    /// A published Release.
    Release,
    /// A version tag, with or without a Release.
    Tag,
    /// A version of a package the forge's registry holds for the
    /// repository.
    Package,
    /// A finished CI run: a workflow run, a pipeline, or a check suite or
    /// commit status where the source reads checks.
    Run,
    /// A security alert: code scanning's, Dependabot's or secret
    /// scanning's, or a GitLab vulnerability.
    Alert,
    /// A pull request, or a GitLab merge request, opened, readied for
    /// review, merged or closed.
    #[serde(rename = "pull-request")]
    PullRequest,
    /// An issue opened, closed or reopened.
    Issue,
    /// A milestone opened, closed, reopened or deleted.
    Milestone,
}

impl Signal {
    /// The signals a source announces where `signals` is absent.
    pub const DEFAULT: [Self; 1] = [Self::Release];
    /// Every signal.
    pub const ALL: [Self; 8] = [
        Self::Release,
        Self::Tag,
        Self::Package,
        Self::Run,
        Self::Alert,
        Self::PullRequest,
        Self::Issue,
        Self::Milestone,
    ];

    /// Its name, as `signals` writes it and an event's `signal` attribute
    /// holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Tag => "tag",
            Self::Package => "package",
            Self::Run => "run",
            Self::Alert => "alert",
            Self::PullRequest => "pull-request",
            Self::Issue => "issue",
            Self::Milestone => "milestone",
        }
    }

    /// The kind of event it announces.
    #[must_use]
    pub const fn kind(self) -> Kind {
        match self {
            Self::Release | Self::Tag => Kind::RELEASE,
            Self::Package => Kind::PACKAGE_VERSION,
            Self::Run => Kind::RUN,
            Self::Alert => Kind::ALERT,
            Self::PullRequest => Kind::PULL_REQUEST,
            Self::Issue => Kind::ISSUE,
            Self::Milestone => Kind::MILESTONE,
        }
    }

    /// The signal named `name`, where it names one.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|signal| signal.name() == name)
    }
}

/// How a source tells a pre-release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PrereleaseRule {
    /// The forge's pre-release flag, or a pre-release part in the version.
    #[default]
    Either,
    /// The forge's pre-release flag alone; a tag, which carries none, is
    /// never a pre-release.
    Flag,
    /// A pre-release part in the version alone, as in `v1.2.0-rc.1`.
    Tag,
}

impl PrereleaseRule {
    /// Whether `version`, flagged as `flag` where the forge flags it, is a
    /// pre-release.
    #[must_use]
    pub fn marks(self, flag: Option<bool>, version: &str) -> bool {
        let flagged = flag == Some(true);
        match self {
            Self::Either => flagged || prerelease_version(version),
            Self::Flag => flagged,
            Self::Tag => prerelease_version(version),
        }
    }
}

/// Which of a tag and its Release a source announces where it announces
/// both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Prefer {
    /// The Release: a tag with none waits for it, as long as the tag window.
    #[default]
    Release,
    /// Whichever comes first.
    First,
}

/// The validators a server last returned for one request.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Validators {
    /// The `ETag`.
    pub etag: Option<String>,
    /// The `Last-Modified`.
    pub last_modified: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_signal_is_named_as_signals_writes_it() {
        for (signal, name, kind) in [
            (Signal::Release, "release", Kind::RELEASE),
            (Signal::Tag, "tag", Kind::RELEASE),
            (Signal::Package, "package", Kind::PACKAGE_VERSION),
            (Signal::Run, "run", Kind::RUN),
            (Signal::Alert, "alert", Kind::ALERT),
            (Signal::PullRequest, "pull-request", Kind::PULL_REQUEST),
            (Signal::Issue, "issue", Kind::ISSUE),
            (Signal::Milestone, "milestone", Kind::MILESTONE),
        ] {
            assert_eq!(signal.name(), name);
            assert_eq!(signal.kind(), kind);
            assert_eq!(Signal::named(name), Some(signal));
            assert_eq!(
                serde_json::to_value(signal).unwrap(),
                serde_json::json!(name)
            );
        }
        assert_eq!(Signal::named("Release"), None);
        assert_eq!(Signal::DEFAULT, [Signal::Release]);
    }

    #[test]
    fn each_rule_tells_a_pre_release_its_own_way() {
        let cases = [
            (Some(true), "v1.0.0", [true, true, false]),
            (Some(false), "v1.0.0-rc.1", [true, false, true]),
            (None, "v1.0.0-rc.1", [true, false, true]),
            (Some(false), "v1.0.0", [false, false, false]),
            (None, "v1.0.0", [false, false, false]),
        ];
        for (flag, version, marked) in cases {
            let rules = [
                PrereleaseRule::Either,
                PrereleaseRule::Flag,
                PrereleaseRule::Tag,
            ];
            assert_eq!(
                rules.map(|rule| rule.marks(flag, version)),
                marked,
                "{flag:?} {version}"
            );
        }
        assert_eq!(PrereleaseRule::default(), PrereleaseRule::Either);
        assert_eq!(Prefer::default(), Prefer::Release);
    }
}
