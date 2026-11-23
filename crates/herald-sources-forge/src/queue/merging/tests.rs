use super::*;

#[test]
fn each_github_entry_state_is_named_and_an_unknown_one_waits() {
    let read: Vec<QueueState> = [
        "QUEUED",
        "AWAITING_CHECKS",
        "MERGEABLE",
        "LOCKED",
        "UNMERGEABLE",
        "NEW",
    ]
    .into_iter()
    .map(state)
    .collect();
    assert_eq!(
        read,
        [
            QueueState::Queued,
            QueueState::Checking,
            QueueState::Mergeable,
            QueueState::Merging,
            QueueState::Unmergeable,
            QueueState::Queued,
        ]
    );
}

#[test]
fn each_github_dequeue_reason_is_named_ignoring_case() {
    let read: Vec<Dequeued> = [
        "MERGE",
        "already_merged",
        "MANUAL",
        "QUEUE_CLEARED",
        "ROLL_BACK",
        "CI_FAILURE",
        "CI_TIMEOUT",
        "MERGE_CONFLICT",
        "GIT_TREE_INVALID",
        "INVALID_MERGE_COMMIT",
        "BRANCH_PROTECTIONS",
        "UNKNOWN_REMOVAL_REASON",
    ]
    .into_iter()
    .map(dequeued)
    .collect();
    assert_eq!(
        read,
        [
            Dequeued::Merged,
            Dequeued::Merged,
            Dequeued::Removed,
            Dequeued::Cleared,
            Dequeued::Cleared,
            Dequeued::ChecksFailed,
            Dequeued::ChecksTimedOut,
            Dequeued::Conflict,
            Dequeued::Invalid,
            Dequeued::Invalid,
            Dequeued::Protections,
            Dequeued::Unknown,
        ]
    );
}

#[test]
fn a_merge_group_s_head_ref_names_its_pull_request() {
    assert_eq!(grouped("gh-readonly-queue/main/pr-42-abc123"), Some(42));
    assert_eq!(
        grouped("refs/heads/gh-readonly-queue/main/pr-43-abc123"),
        Some(43)
    );
    assert_eq!(
        grouped("gh-readonly-queue/release/2.x/pr-7-def456"),
        Some(7)
    );
    for unnamed in [
        "refs/heads/main",
        "refs/heads/feature/pr-5-abc",
        "gh-readonly-queue/pr-42-abc",
        "gh-readonly-queue/main/42-abc",
        "gh-readonly-queue/main/pr-42",
        "gh-readonly-queue/main/pr-x-abc",
    ] {
        assert_eq!(grouped(unnamed), None);
    }
}
