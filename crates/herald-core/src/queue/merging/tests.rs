use super::*;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

fn left(reason: Dequeued) -> Left {
    Left { at: at(9), reason }
}

#[test]
fn every_state_and_reason_has_a_name() {
    let states = [
        QueueState::Queued,
        QueueState::Checking,
        QueueState::Mergeable,
        QueueState::Merging,
        QueueState::Unmergeable,
    ];
    let names: Vec<&str> = states.iter().map(|state| state.name()).collect();
    assert_eq!(
        names,
        ["queued", "checking", "mergeable", "merging", "unmergeable"]
    );
    for state in states {
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!(state.name())
        );
    }
    let names: Vec<&str> = Dequeued::ALL.iter().map(|reason| reason.name()).collect();
    assert_eq!(
        names,
        [
            "checks-failed",
            "checks-timed-out",
            "conflict",
            "invalid",
            "protections",
            "removed",
            "cleared",
            "merged",
            "unknown",
        ]
    );
    for reason in Dequeued::ALL {
        assert_eq!(
            serde_json::to_value(reason).unwrap(),
            serde_json::json!(reason.name())
        );
    }
}

#[test]
fn only_a_failure_of_the_pull_request_is_a_failure() {
    let failures: Vec<Dequeued> = Dequeued::ALL
        .into_iter()
        .filter(|reason| reason.failure())
        .collect();
    assert_eq!(failures, Dequeued::FAILURES);
    assert!(!Dequeued::Merged.failure() && !Dequeued::Unknown.failure());
}

#[test]
fn an_entry_waits_until_it_leaves_and_leaving_keeps_when_it_entered() {
    let entered = QueueEntry {
        position: Some(2),
        state: QueueState::Checking,
        since: Some(at(5)),
        estimate: Some(60),
        left: None,
    };
    assert!(entered.waiting());
    let gone = QueueEntry::leaving(Some(&entered), left(Dequeued::ChecksFailed));
    assert!(!gone.waiting());
    assert_eq!(
        gone,
        QueueEntry {
            position: None,
            state: QueueState::Checking,
            since: Some(at(5)),
            estimate: None,
            left: Some(left(Dequeued::ChecksFailed)),
        }
    );
    let unknown = QueueEntry::leaving(None, left(Dequeued::Removed));
    assert_eq!(
        (unknown.state, unknown.since, unknown.left),
        (QueueState::Queued, None, Some(left(Dequeued::Removed)))
    );
}

#[test]
fn a_reread_keeps_when_a_waiting_entry_entered_and_why_one_left() {
    let waiting = QueueEntry::entered(Some(at(5)));
    let bare = QueueEntry::entered(None);
    assert_eq!(
        QueueEntry::reread(Some(&waiting), Some(bare.clone())),
        Some(waiting.clone())
    );
    let gone = QueueEntry::leaving(Some(&waiting), left(Dequeued::Conflict));
    assert_eq!(
        QueueEntry::reread(Some(&gone), Some(bare.clone())),
        Some(bare.clone())
    );
    assert_eq!(QueueEntry::reread(Some(&gone), None), Some(gone.clone()));
    assert_eq!(QueueEntry::reread(Some(&waiting), None), None);
    assert_eq!(
        QueueEntry::reread(None, Some(bare.clone())),
        Some(bare.clone())
    );
    let read = QueueEntry::entered(Some(at(7)));
    assert_eq!(
        QueueEntry::reread(Some(&waiting), Some(read.clone())),
        Some(read)
    );
    assert_eq!(QueueEntry::reread(None, None), None);
}
