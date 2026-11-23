//! What GitHub says of a pull request's merge queue entry, as herald
//! names it (ADR-0078).

use herald_core::{Dequeued, QueueState};

/// The state GitHub's `MergeQueueEntryState` named `state` is; one herald
/// does not know is taken as waiting its turn.
pub(crate) fn state(state: &str) -> QueueState {
    match state {
        "AWAITING_CHECKS" => QueueState::Checking,
        "MERGEABLE" => QueueState::Mergeable,
        "LOCKED" => QueueState::Merging,
        "UNMERGEABLE" => QueueState::Unmergeable,
        _ => QueueState::Queued,
    }
}

/// Why GitHub's `dequeued` delivery says a pull request left the queue,
/// ignoring case.
pub(crate) fn dequeued(reason: &str) -> Dequeued {
    match reason.to_ascii_uppercase().as_str() {
        "MERGE" | "ALREADY_MERGED" => Dequeued::Merged,
        "MANUAL" => Dequeued::Removed,
        "QUEUE_CLEARED" | "ROLL_BACK" => Dequeued::Cleared,
        "CI_FAILURE" => Dequeued::ChecksFailed,
        "CI_TIMEOUT" => Dequeued::ChecksTimedOut,
        "MERGE_CONFLICT" => Dequeued::Conflict,
        "GIT_TREE_INVALID" | "INVALID_MERGE_COMMIT" => Dequeued::Invalid,
        "BRANCH_PROTECTIONS" => Dequeued::Protections,
        _ => Dequeued::Unknown,
    }
}

/// The number of the pull request a merge group's head ref is of:
/// `gh-readonly-queue/<base>/pr-<number>-<sha>`, under `refs/heads/` or
/// not.
pub(crate) fn grouped(head_ref: &str) -> Option<u64> {
    let branch = head_ref.strip_prefix("refs/heads/").unwrap_or(head_ref);
    let rest = branch.strip_prefix("gh-readonly-queue/")?;
    let (_, last) = rest.rsplit_once('/')?;
    let (number, _) = last.strip_prefix("pr-")?.split_once('-')?;
    number.parse().ok()
}

#[cfg(test)]
mod tests;
