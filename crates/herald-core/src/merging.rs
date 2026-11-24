//! A merge queue's settings, and the write credential a source or a queue
//! writes with (ADR-0068, ADR-0087).

use std::num::{NonZeroU8, NonZeroU64};

use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{SecretRef, kind_name};

pub use verdict::{Check, CheckState, Heads, Left, Read, Verdict, unvouched, verdict, vouched};

/// The label a pull request is queued by where `enter` is absent.
pub const ENTER_LABEL: &str = "herald:merge";

/// The most pull requests one queue's read takes in, oldest gesture
/// first.
pub const MOST_ENTERED: usize = 50;

/// How long herald waits for a head's checks where `wait` is absent.
pub const WAIT: SignedDuration = SignedDuration::from_hours(2);

/// How far apart a forge's stamps of a gesture and of a push may be where
/// `skew` is absent: a push stamped this close before a gesture counts as
/// after it.
pub const SKEW: SignedDuration = SignedDuration::from_secs(5);

/// Which head a gesture vouches for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Vouches {
    /// The head the pull request had at the gesture's time on the forge,
    /// by the forge's own record of pushes to its branch.
    #[default]
    Gesture,
    /// The head herald first reads with the gesture standing.
    Seen,
    /// The head herald first reads with the gesture standing, once one who
    /// holds the queue's least role approved that commit on the forge.
    Approved,
}

/// Who runs a branch's queue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(rename = "MergeRuns")]
pub enum Runs {
    /// The forge's own merge queue or merge train.
    #[default]
    Forge,
    /// herald's own serial queue.
    Herald,
}

impl Runs {
    /// The setting as the config writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Forge => "forge",
            Self::Herald => "herald",
        }
    }
}

/// The gesture that queues a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Enter {
    /// A label of this name added to the pull request.
    Label(String),
    /// A comment on the pull request whose whole body, trimmed, is this
    /// command.
    Comment(String),
}

impl Default for Enter {
    fn default() -> Self {
        Self::Label(ENTER_LABEL.to_owned())
    }
}

/// How a queue merges a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Method {
    /// A merge commit.
    Merge,
    /// One squashed commit.
    Squash,
    /// The commits rebased onto the base.
    Rebase,
    /// The base moved to the head, with no new commit.
    FastForward,
    /// The repository's one allowed method.
    Repository,
}

impl Method {
    /// The method as the config writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
            Self::FastForward => "fast-forward",
            Self::Repository => "repository",
        }
    }

    /// Whether the commit it makes carries a message of herald's choosing.
    #[must_use]
    pub const fn messaged(self) -> bool {
        matches!(self, Self::Merge | Self::Squash)
    }
}

/// The commit message a merge is made with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Message {
    /// The forge's own for the method.
    #[default]
    Forge,
    /// The pull request's title and body.
    Pull,
}

/// Where a queue's steps are taken.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AppliedBy {
    /// By `herald merge-queue --apply`.
    #[default]
    Command,
    /// By `herald server`, each round and as webhooks arrive.
    Server,
}

/// How a pull request's branch is brought up to date with its base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Update {
    /// The base merged into the branch.
    Merge,
    /// The branch rebased onto the base.
    Rebase,
}

impl Update {
    /// The style as the config writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Rebase => "rebase",
        }
    }
}

/// The failed checks a queue re-runs before a pull request leaves.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Retries {
    /// The checks, by name, whose failed job is re-run.
    pub checks: Vec<String>,
    /// The most re-runs on one head.
    #[schemars(with = "NonZeroU8")]
    pub times: NonZeroU8,
}

/// A write credential, apart from the one a source reads with (ADR-0068).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Write {
    /// A token that may make the writes, other than the one the source
    /// reads with.
    pub token: Option<SecretRef>,
    /// The GitHub App the writes are made as, by the name of its
    /// `[github_apps.<name>]` table, in place of `token`: an App other
    /// than the one the source reads as, installed with the permissions
    /// its writes need alone.
    pub app: Option<String>,
    /// The id of the App's installation tokens are minted for; looked up
    /// on the owner, or on the repository, where absent.
    #[schemars(with = "Option<NonZeroU64>")]
    pub installation: Option<NonZeroU64>,
}

impl Write {
    /// Whether it names one token or one App, by an App's name.
    ///
    /// # Errors
    ///
    /// Where it names both or neither, an installation beside a token, or
    /// an App by what is no App's name.
    pub fn check(&self) -> Result<(), String> {
        match (&self.token, &self.app, self.installation) {
            (Some(_), Some(_), _) | (None, None, _) => {
                Err("a write credential is a token or a GitHub App, one of them; give write.token or write.app".to_owned())
            }
            (Some(_), None, Some(_)) => Err("write.installation names the App's installation a token is minted for; give write.app, or remove write.installation".to_owned()),
            (None, Some(app), _) if !kind_name(app) => Err(format!(
                "\"{app}\" is not an App's name; give the name of its [github_apps.<name>] table"
            )),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
mod verdict;
