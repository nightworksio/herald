//! The merge queues, each a base branch whose queued pull requests the
//! forge's merge queue or herald's own merges (ADR-0087).

use herald_core::merging::{AppliedBy, Enter, Message, Method, Retries, Runs, Vouches, Write};
use herald_core::{Duration, Installation, Permission, SourceName};
use herald_kinds::{Installed, Merging, WriteCredential};
use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::Deserialize;
use toml::Spanned;

/// A `[merge_queues.<name>]` table, as written.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "MergeQueue")]
pub(crate) struct MergeQueueFile {
    /// The source whose repository it merges in: one of kind "github",
    /// "gitlab" or "forgejo".
    #[schemars(with = "String")]
    pub(crate) source: Spanned<String>,
    /// The repository it merges in, by full path, where the source is an
    /// owner; the source's own where absent.
    pub(crate) repository: Option<String>,
    /// The base branch it merges into; one table per branch.
    pub(crate) branch: String,
    /// Who runs it: "forge" hands each pull request to the forge's merge
    /// queue or merge train; "herald" runs herald's own serial queue, also
    /// where the forge has one. "forge" where absent.
    pub(crate) runs: Option<Runs>,
    /// The gesture that queues a pull request: `{ label = "<name>" }`, or
    /// `{ comment = "<command>" }`, a comment whose whole body is the
    /// command; `{ label = "herald:merge" }` where absent.
    pub(crate) enter: Option<Enter>,
    /// The least role the person who queues holds on the repository:
    /// "write" (GitHub write, GitLab Developer, Forgejo write, or above),
    /// "maintain" or "admin"; "write" where absent.
    pub(crate) least: Option<Permission>,
    /// Which head a gesture vouches for: "gesture", the head at the
    /// gesture's time on the forge; "seen", the head herald first reads
    /// with the gesture; "approved", that head once one who holds `least`
    /// approved it on the forge. "gesture" where absent.
    pub(crate) vouches: Option<Vouches>,
    /// How far apart the forge's stamps of a gesture and of a push may be,
    /// its services' clocks never quite agreeing: a push stamped up to this
    /// before a gesture, or before a GitLab approval, counts as after it,
    /// so the gesture vouches for no head that push may have brought; "5s"
    /// where absent.
    pub(crate) skew: Option<Duration>,
    /// How it merges: "merge", "squash", "rebase", "fast-forward", or
    /// "repository", the repository's one allowed method. A queue without
    /// it does not start.
    pub(crate) method: Option<Method>,
    /// The commit message: "forge", the forge's own for the method, or
    /// "pull", the pull request's title and body, with "merge" or "squash"
    /// alone; "forge" where absent.
    pub(crate) message: Option<Message>,
    /// The checks, by name, that may be red or missing on the branch;
    /// never one the forge requires.
    #[serde(default)]
    pub(crate) may_fail: Vec<String>,
    /// How long it waits for a head's checks before the pull request
    /// leaves; "2h" where absent.
    pub(crate) wait: Option<Duration>,
    /// The checks whose failed job is re-run, at most `times` on one head,
    /// before the pull request leaves; none where absent, and none on
    /// Forgejo.
    pub(crate) retries: Option<Retries>,
    /// Where its steps are taken: "command", by `herald merge-queue
    /// --apply`; "server", by `herald server`, which then reads the write
    /// credential. "command" where absent.
    pub(crate) applied_by: Option<AppliedBy>,
    /// The credential its steps are made with, held apart from every one
    /// herald reads with: `{ token = { secret = "NAME" } }` or `{ app =
    /// "<github_apps name>" }`, the same as its source's `write` or another.
    /// The queue does not start without it.
    pub(crate) write: Option<Write>,
}

/// A merge queue, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeQueue {
    /// The source whose repository it merges in.
    pub source: SourceName,
    /// What the source gives it: the forge, repository, API and update
    /// style.
    pub forge: Merging,
    /// The base branch it merges into.
    pub branch: String,
    /// Who runs it.
    pub runs: Runs,
    /// The gesture that queues a pull request.
    pub enter: Enter,
    /// The least role the person who queues holds.
    pub least: Permission,
    /// Which head a gesture vouches for.
    pub vouches: Vouches,
    /// How far apart the forge's stamps of a gesture and of a push may be.
    pub skew: SignedDuration,
    /// How it merges; it does not start without one.
    pub method: Option<Method>,
    /// The commit message it merges with.
    pub message: Message,
    /// The checks that may be red or missing.
    pub may_fail: Vec<String>,
    /// How long it waits for a head's checks.
    pub wait: SignedDuration,
    /// The failed checks it re-runs, where it re-runs any.
    pub retries: Option<Retries>,
    /// Where its steps are taken.
    pub applied_by: AppliedBy,
    /// The credential its steps are made with; it does not start without
    /// one.
    pub write: Option<Write>,
}

impl MergeQueue {
    /// The credential its steps are made with, sent to its forge's API
    /// alone, where it names one.
    #[must_use]
    pub fn writing(&self) -> Option<WriteCredential<'_>> {
        let write = self.write.as_ref()?;
        let api = self.forge.api.clone();
        if let Some(secret) = &write.token {
            return Some(WriteCredential::Token { secret, api });
        }
        Some(WriteCredential::App(Installed {
            app: write.app.as_deref()?,
            api,
            installation: write
                .installation
                .map_or(Installation::Repository(&self.forge.repository), |id| {
                    Installation::Fixed(id.get())
                }),
        }))
    }
}
