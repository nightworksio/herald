use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::version::prerelease_version;

/// The attribute naming the signal an event is of.
pub const SIGNAL: &str = "signal";

/// The attribute marking an event as an edit of one the forge published
/// before (ADR-0034).
pub const EDITED: &str = "edited";

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
}

impl Signal {
    /// The signals a source announces where `signals` is absent.
    pub const DEFAULT: [Self; 1] = [Self::Release];
    /// Every signal.
    pub const ALL: [Self; 3] = [Self::Release, Self::Tag, Self::Package];

    /// Its name, as `signals` writes it and an event's `signal` attribute
    /// holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Tag => "tag",
            Self::Package => "package",
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
        for (signal, name) in [
            (Signal::Release, "release"),
            (Signal::Tag, "tag"),
            (Signal::Package, "package"),
        ] {
            assert_eq!(signal.name(), name);
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
