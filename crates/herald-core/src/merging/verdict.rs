//! What herald's own queue does next with a queued pull request, from what
//! it read of it: leave, bring it up to date, wait, or merge (ADR-0087).

use jiff::SignedDuration;
use serde::{Deserialize, Serialize};

use super::Vouches;

/// Why a pull request leaves herald's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Left {
    /// A check failed.
    ChecksFailed,
    /// Its checks outlasted the queue's `wait`.
    ChecksTimedOut,
    /// Bringing it up to date conflicted.
    Conflict,
    /// It could not be brought up to date.
    CannotUpdate,
    /// Its head moved past the one the gesture vouched for.
    HeadMoved,
    /// Its gesture was taken back.
    Removed,
    /// The forge says it cannot merge.
    Unmergeable,
    /// The forge refused the merge.
    MergeRefused,
    /// What decides whether it stands or is green could not be read whole.
    Unread,
    /// Its gesture vouches for the head herald first saw with it, and no
    /// record of that head is kept.
    Unrecorded,
}

impl Left {
    /// Whether it is a failure of the pull request, which a rule's
    /// `dequeued` tells of: every reason but a gesture taken back.
    #[must_use]
    pub const fn failed(self) -> bool {
        !matches!(self, Self::Removed)
    }
}

/// How a check on a head stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// Still running, or queued.
    Pending,
    /// Passed, or skipped or neutral where the forge counts that as passing.
    Passed,
    /// Failed, cancelled or timed out.
    Failed,
}

/// A check on a head, by the name the forge shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Its name.
    pub name: String,
    /// How it stands.
    pub state: CheckState,
}

/// What the forge's records say of the heads a gesture may vouch for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Heads<'a> {
    /// The head the gesture was made on, by the forge's record of pushes;
    /// none where the forge holds none.
    pub then: Option<&'a str>,
    /// The commit the gesture's comment named by its full id, where it
    /// named one.
    pub named: Option<&'a str>,
    /// The head herald first read with the gesture standing.
    pub seen: Option<&'a str>,
    /// Whether one who holds the queue's least role approved the head now
    /// on the forge.
    pub approved: bool,
}

/// What herald read of a queued pull request.
#[derive(Debug, Clone, Copy)]
pub struct Read<'a> {
    /// Whether its gesture still stands, by a person who still holds the
    /// least role.
    pub entered: bool,
    /// Which head its gesture vouches for, by the queue's setting.
    pub vouches: Vouches,
    /// What the forge's records say of the heads its gesture may vouch
    /// for.
    pub heads: Heads<'a>,
    /// Its head now.
    pub head: &'a str,
    /// Whether its head holds the base's head.
    pub up_to_date: bool,
    /// Whether the forge says its head merges into the base's head without
    /// a conflict; none while the forge works it out.
    pub clean: Option<bool>,
    /// The checks reported on its head.
    pub checks: &'a [Check],
    /// The checks the forge requires on the base.
    pub required: &'a [String],
    /// Whether the forge says it can merge; none while the forge works it
    /// out.
    pub mergeable: Option<bool>,
    /// How long since its head was proved up to date.
    pub waited: SignedDuration,
    /// Whether its gesture's record, its checks and the checks its base
    /// requires were each read whole.
    pub whole: bool,
}

/// What herald's queue does next with a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// It leaves the queue.
    Leaves(Left),
    /// Its branch is brought up to date with the base.
    Update,
    /// Its checks or the forge's answer are waited for.
    Wait,
    /// It is merged, with its head as the expected head.
    Merge,
    /// The failed job of a check the queue's `retries` names is re-run.
    Rerun,
}

/// Whether a gesture of which `heads` say what they say vouches for the
/// head `head` under `vouches`. A commit the gesture's comment named stands
/// in for the record of the head (the push record, or herald's record of
/// the head it first saw), never for the approval:
///
/// | `vouches` | the head on record is | and also |
/// |-----------|-----------------------|----------|
/// | `Gesture` | the commit named, else the head the push record gives | |
/// | `Seen` | the commit named, else the head first seen | |
/// | `Approved` | the commit named, else the head first seen | approved |
///
/// It vouches only where the head on record is `head`; a head no record
/// gives is vouched for by none.
#[must_use]
pub fn vouched(vouches: Vouches, heads: &Heads<'_>, head: &str) -> bool {
    let recorded = heads.named.or(match vouches {
        Vouches::Gesture => heads.then,
        Vouches::Seen | Vouches::Approved => heads.seen,
    });
    recorded == Some(head) && (vouches != Vouches::Approved || heads.approved)
}

/// Why a gesture of which `heads` say what they say does not vouch for
/// `head` under `vouches`, where it does not: no record of the head first
/// seen is kept, or the head moved past the one vouched for.
#[must_use]
pub fn unvouched(vouches: Vouches, heads: &Heads<'_>, head: &str) -> Option<Left> {
    if vouched(vouches, heads, head) {
        return None;
    }
    let unrecorded = vouches != Vouches::Gesture && heads.named.is_none() && heads.seen.is_none();
    Some(if unrecorded {
        Left::Unrecorded
    } else {
        Left::HeadMoved
    })
}

/// The checks reported on `read`'s head that count, on a queue that lets
/// the checks of `may_fail` be red or missing unless the forge requires
/// them.
fn counting<'r>(read: &Read<'r>, may_fail: &[String]) -> Vec<&'r Check> {
    let required = |name: &str| read.required.iter().any(|check| check == name);
    let counted = |check: &&Check| required(&check.name) || !may_fail.contains(&check.name);
    read.checks.iter().filter(counted).collect()
}

/// The names of the checks that count and failed on `read`'s head, on a
/// queue that lets the checks of `may_fail` be red or missing unless the
/// forge requires them.
#[must_use]
pub fn failing<'r>(read: &Read<'r>, may_fail: &[String]) -> Vec<&'r str> {
    counting(read, may_fail)
        .into_iter()
        .filter(|check| check.state == CheckState::Failed)
        .map(|check| check.name.as_str())
        .collect()
}

/// What comes next for the pull request `read`, on a queue that lets the
/// checks of `may_fail` be red or missing, unless the forge requires them,
/// and waits `wait` for its checks: a head on which no check is reported
/// yet is waited for, never merged, and a branch is brought up to date only
/// where the forge says it merges into the base without a conflict.
#[must_use]
pub fn verdict(read: &Read<'_>, may_fail: &[String], wait: SignedDuration) -> Verdict {
    if !read.entered {
        return Verdict::Leaves(Left::Removed);
    }
    if !read.whole {
        return Verdict::Leaves(Left::Unread);
    }
    if let Some(left) = unvouched(read.vouches, &read.heads, read.head) {
        return Verdict::Leaves(left);
    }
    if !read.up_to_date {
        return match read.clean {
            Some(true) => Verdict::Update,
            Some(false) => Verdict::Leaves(Left::Conflict),
            None if read.waited >= wait => Verdict::Leaves(Left::ChecksTimedOut),
            None => Verdict::Wait,
        };
    }
    if !failing(read, may_fail).is_empty() {
        return Verdict::Leaves(Left::ChecksFailed);
    }
    let counting = counting(read, may_fail);
    let missing = read
        .required
        .iter()
        .any(|name| !read.checks.iter().any(|check| &check.name == name));
    let pending = counting
        .iter()
        .any(|check| check.state == CheckState::Pending);
    let answered = read.mergeable.is_some();
    if missing || pending || !answered || counting.is_empty() {
        return if read.waited >= wait {
            Verdict::Leaves(Left::ChecksTimedOut)
        } else {
            Verdict::Wait
        };
    }
    if read.mergeable == Some(false) {
        return Verdict::Leaves(Left::Unmergeable);
    }
    Verdict::Merge
}

#[cfg(test)]
mod tests;
