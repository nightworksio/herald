use jiff::Timestamp;

use super::*;
use crate::members::Visibility;
use crate::queue::tests::pull;
use crate::queue::{QueueEntry, View, board};

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

fn left(number: u64, reason: Dequeued, second: i64) -> Pull {
    let mut pull = pull();
    pull.number = number;
    pull.queue = Some(QueueEntry::leaving(
        None,
        Left {
            at: at(second),
            reason,
        },
    ));
    pull
}

#[test]
fn a_rule_alerts_on_each_pull_request_that_left_for_a_reason_it_names_the_latest_first() {
    let mut waiting = pull();
    waiting.number = 9;
    waiting.queue = Some(QueueEntry::entered(None));
    let pulls = [
        left(1, Dequeued::ChecksFailed, 3_000),
        left(2, Dequeued::Merged, 4_000),
        left(3, Dequeued::Conflict, 5_000),
        left(4, Dequeued::Conflict, 5_000),
        left(5, Dequeued::Removed, 6_000),
        waiting,
        pull(),
    ];
    let shown = board(&View::default(), &pulls, at(10_000));
    let alerted: Vec<u64> = dequeues(&shown, &Dequeued::FAILURES, 10)
        .into_iter()
        .map(|dequeue| dequeue.pull.number)
        .collect();
    assert_eq!(alerted, [3, 4, 1]);
    let most: Vec<u64> = dequeues(&shown, &Dequeued::FAILURES, 2)
        .into_iter()
        .map(|dequeue| dequeue.pull.number)
        .collect();
    assert_eq!(most, [3, 4]);
    let removed: Vec<u64> = dequeues(&shown, &[Dequeued::Removed], 10)
        .into_iter()
        .map(|dequeue| dequeue.pull.number)
        .collect();
    assert_eq!(removed, [5]);
}

#[test]
fn an_alert_is_an_event_of_its_rule_pull_and_time_left_seen_as_its_repository() {
    let mut pull = left(7, Dequeued::ChecksTimedOut, 3_000);
    pull.visibility = Visibility::Internal;
    let dequeue = Dequeue {
        pull: &pull,
        left: pull.queue.as_ref().unwrap().left.unwrap(),
    };
    let event = dequeue_event("queue", dequeue).unwrap();
    assert_eq!(event.kind(), &Kind::DEQUEUED);
    assert_eq!(
        dequeue_identity("queue", dequeue),
        "rule/queue/github.com/nightworksio/herald/pull/7/dequeued/1970-01-01T00:50:00Z"
    );
    assert_eq!(event.visibility(), Some(Visibility::Internal));
    assert_eq!(event.occurred(), at(3_000));
    assert_eq!(
        event.attributes().get(REASON),
        Some(&Attribute::Text("checks-timed-out".to_owned()))
    );
    assert_eq!(
        event.attributes().get(FAILURE),
        Some(&Attribute::Flag(true))
    );
    assert_eq!(
        event.attributes().get(AUTHOR),
        Some(&Attribute::Text("wessel".to_owned()))
    );
    assert!(Kind::DEQUEUED.own());
    let mut anonymous = left(8, Dequeued::Removed, 3_000);
    anonymous.author = None;
    let dequeue = Dequeue {
        pull: &anonymous,
        left: anonymous.queue.as_ref().unwrap().left.unwrap(),
    };
    let event = dequeue_event("queue", dequeue).unwrap();
    assert_eq!(event.attributes().get(AUTHOR), None);
    assert_eq!(
        event.attributes().get(FAILURE),
        Some(&Attribute::Flag(false))
    );
    let specs = crate::kind::KindSpecs::default();
    let spec = specs.get(&Kind::DEQUEUED).unwrap();
    assert_eq!(spec.check(event.attributes()), Ok(()));
}
