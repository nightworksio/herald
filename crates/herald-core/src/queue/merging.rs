//! Where an open pull request stands in its forge's merge queue or merge
//! train, and why it last left it (ADR-0078).

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What the forge says of a pull request's entry in its queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueueState {
    /// It waits its turn.
    Queued,
    /// Its checks run on the queue's merge of it.
    Checking,
    /// Its checks passed and it merges next.
    Mergeable,
    /// The forge is merging it.
    Merging,
    /// It cannot merge as it stands and leaves the queue.
    Unmergeable,
}

impl QueueState {
    /// The state as a row, a template and a page name it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Checking => "checking",
            Self::Mergeable => "mergeable",
            Self::Merging => "merging",
            Self::Unmergeable => "unmergeable",
        }
    }
}

/// Why a pull request left its forge's queue.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Dequeued {
    /// It was merged.
    Merged,
    /// Someone took it out.
    Removed,
    /// The whole queue was emptied or rolled back.
    Cleared,
    /// A required check failed on the queue's merge of it.
    ChecksFailed,
    /// A required check did not finish in time.
    ChecksTimedOut,
    /// It conflicts with what is ahead of it.
    Conflict,
    /// The forge could not build the merge of it.
    Invalid,
    /// Its branch protections no longer let it merge.
    Protections,
    /// The forge named no reason herald knows.
    Unknown,
}

impl Dequeued {
    /// Every reason, in the order a rule's setting lists them.
    pub const ALL: [Self; 9] = [
        Self::ChecksFailed,
        Self::ChecksTimedOut,
        Self::Conflict,
        Self::Invalid,
        Self::Protections,
        Self::Removed,
        Self::Cleared,
        Self::Merged,
        Self::Unknown,
    ];

    /// The reasons that are a failure of the pull request: those a rule
    /// alerts on unless it names others.
    pub const FAILURES: [Self; 5] = [
        Self::ChecksFailed,
        Self::ChecksTimedOut,
        Self::Conflict,
        Self::Invalid,
        Self::Protections,
    ];

    /// The reason as a row, a rule and an event name it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Merged => "merged",
            Self::Removed => "removed",
            Self::Cleared => "cleared",
            Self::ChecksFailed => "checks-failed",
            Self::ChecksTimedOut => "checks-timed-out",
            Self::Conflict => "conflict",
            Self::Invalid => "invalid",
            Self::Protections => "protections",
            Self::Unknown => "unknown",
        }
    }

    /// Whether it is a failure of the pull request.
    #[must_use]
    pub fn failure(self) -> bool {
        Self::FAILURES.contains(&self)
    }
}

/// When and why a pull request last left its forge's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Left {
    /// When.
    pub at: Timestamp,
    /// Why.
    pub reason: Dequeued,
}

/// A pull request's entry in its forge's merge queue or merge train, or
/// its last one where it left (ADR-0078).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEntry {
    /// Its place in the queue, the first being 1, where the forge says.
    pub position: Option<u64>,
    /// What the forge says of it.
    pub state: QueueState,
    /// When it entered the queue, where the forge says.
    pub since: Option<Timestamp>,
    /// How many seconds the forge estimates until it merges, where it
    /// does.
    pub estimate: Option<u64>,
    /// When and why it left the queue, where it did.
    pub left: Option<Left>,
}

impl QueueEntry {
    /// An entry the forge says only exists, entered `since` where known.
    #[must_use]
    pub const fn entered(since: Option<Timestamp>) -> Self {
        Self {
            position: None,
            state: QueueState::Queued,
            since,
            estimate: None,
            left: None,
        }
    }

    /// Whether the pull request is in the queue now.
    #[must_use]
    pub const fn waiting(&self) -> bool {
        self.left.is_none()
    }

    /// The entry `kept` became where it left the queue as `left` says.
    #[must_use]
    pub fn leaving(kept: Option<&Self>, left: Left) -> Self {
        let mut entry = kept.cloned().unwrap_or_else(|| Self::entered(None));
        entry.position = None;
        entry.estimate = None;
        entry.left = Some(left);
        entry
    }

    /// What a full read saying `read` makes of the entry `kept`: an entry
    /// read keeps the time it entered from `kept` where the read names
    /// none and `kept` was waiting; where the read names none, only the
    /// record of why it last left is kept.
    #[must_use]
    pub fn reread(kept: Option<&Self>, read: Option<Self>) -> Option<Self> {
        match (kept, read) {
            (Some(kept), Some(mut read)) => {
                if kept.waiting() {
                    read.since = read.since.or(kept.since);
                }
                Some(read)
            }
            (Some(kept), None) => kept.left.is_some().then(|| kept.clone()),
            (None, read) => read,
        }
    }
}

#[cfg(test)]
mod tests;
