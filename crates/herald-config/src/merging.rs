//! The merge queues, each a base branch whose queued pull requests the
//! forge's merge queue or herald's own merges (ADR-0087).

use herald_core::merging::{
    AppliedBy, Enter, Message, Method, Retries, RoleLost, Runs, UPDATE_WAIT_MOST, Updated, Vouches,
    Write,
};
use herald_core::{Duration, Installation, Lasting, Permission, SourceName};
use herald_kinds::{Installed, Merging, WriteCredential};
use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::Deserialize;
use toml::Spanned;

/// The kind of a GitHub source, as a merge queue's forge names it.
const GITHUB: &str = "github";

/// Why `signer` is refused where the forge is not GitHub, or it is blank.
pub(crate) const UNSIGNING: &str = "signer names the committer a GitHub Enterprise Server or GHE.com instance signs the commits it makes for, such as signer = \"noreply@ghe.example\"; GitLab and Forgejo give no such signature, so remove signer there";

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
    /// Whether a gesture vouches for the head herald's own update made from
    /// the head it vouched for: "signed", where the new head's parents are
    /// exactly that head and the base's head herald recorded, and the forge
    /// itself signed it; "never", the pull request leaves and a new gesture
    /// queues it; "approved", once one who holds `least` approved the new
    /// head. "signed" where absent; herald's own queue alone.
    pub(crate) updated: Option<Updated>,
    /// The committer a GitHub Enterprise Server or GHE.com instance's web
    /// flow signs the commits it makes for, as its signature names it: the
    /// operator vouches that it is the instance's own identity, never a
    /// user's. With `updated = "signed"` it lets a gesture vouch for the
    /// head herald's own update made there; github.com's own,
    /// `noreply@github.com`, where absent there; herald's own queue on GitHub
    /// alone.
    pub(crate) signer: Option<String>,
    /// How long herald pauses in all, at most, for the head of a pull
    /// request it brought up to date to move, where the forge answered
    /// before it moved: herald reads the head right after its update, and
    /// again every 2s while the head has not moved and the pauses stay
    /// within this; "0s" reads it once. "10s" where absent, at most "1m";
    /// herald's own queue alone, read with `updated = "signed"` alone.
    pub(crate) update_wait: Option<Lasting>,
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
    /// What becomes of a pull request herald handed to the forge's queue,
    /// or found already there, once the person whose gesture it is no
    /// longer holds `least` (a lower role, none, or an account since
    /// deleted), whether or not a cancel herald does not carry out stands:
    /// "take-out", taken out on herald's next applying run, which says
    /// why; "stay", left in the forge's queue and said so. Refused with
    /// `runs = "herald"`. "take-out" where absent.
    pub(crate) role_lost: Option<RoleLost>,
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
    /// Whether a gesture vouches for the head herald's own update made.
    pub updated: Updated,
    /// The committer the forge's web flow signs the commits it makes for,
    /// where the config names one.
    pub signer: Option<String>,
    /// How long it pauses in all, at most, for the head of a pull request
    /// it updated to move.
    pub update_wait: SignedDuration,
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
    /// What becomes of a pull request it handed to the forge's queue once
    /// the person whose gesture it handed over lost its least role.
    pub role_lost: RoleLost,
    /// The credential its steps are made with; it does not start without
    /// one.
    pub write: Option<Write>,
}

impl MergeQueue {
    /// Whether a queue on the forge `forge`, as a source's kind names it,
    /// takes a `signer`: GitHub alone signs the commits it makes.
    #[must_use]
    pub fn signs(forge: &str) -> bool {
        forge == GITHUB
    }

    /// Why a queue on the forge `forge` refuses `signer`, where it does:
    /// off GitHub, or blank.
    #[must_use]
    pub fn unsigning(forge: &str, signer: &str) -> Option<&'static str> {
        (!Self::signs(forge) || signer.trim().is_empty()).then_some(UNSIGNING)
    }

    /// Why a queue refuses `update_wait` of `wait`, where it does: longer
    /// than [`UPDATE_WAIT_MOST`].
    #[must_use]
    pub fn unwaited(wait: SignedDuration) -> Option<String> {
        (wait > UPDATE_WAIT_MOST).then(|| {
            let most = Duration(UPDATE_WAIT_MOST).written();
            format!("update_wait is at most \"{most}\"; shorten it")
        })
    }

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
