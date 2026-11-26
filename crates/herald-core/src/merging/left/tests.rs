use jiff::{SignedDuration, Timestamp};

use super::*;
use crate::merging::Leaving;
use crate::queue::Dequeued;

fn at(minutes: i64) -> Timestamp {
    "2026-01-01T00:00:00Z".parse::<Timestamp>().unwrap() + SignedDuration::from_mins(minutes)
}

fn gestured() -> GestureKey {
    GestureKey {
        api: "https://api.github.com".to_owned(),
        repository: "o/r".to_owned(),
        number: 7,
        gesture: 70,
    }
}

fn kept(gesture: u64, minutes: i64) -> LeftRecord {
    LeftRecord {
        key: gestured().pull(),
        gesture,
        left: Left {
            at: at(minutes),
            reason: Dequeued::HeadMoved,
        },
    }
}

#[test]
fn a_gesture_that_left_gives_its_pull_request_when_and_why() {
    let leaving = Leaving {
        left: super::super::Left::CannotUpdate,
        at: at(3),
    };
    let left = LeftRecord::of(&gestured(), leaving);
    assert_eq!(
        left,
        LeftRecord {
            key: LeftKey {
                api: "https://api.github.com".to_owned(),
                repository: "o/r".to_owned(),
                number: 7,
            },
            gesture: 70,
            left: Left {
                at: at(3),
                reason: Dequeued::CannotUpdate,
            },
        }
    );
    assert_eq!(left.key.to_string(), "https://api.github.com/o/r/7");
}

#[test]
fn a_leaving_replaces_only_an_older_one_of_another_gesture_or_time() {
    let leaving = kept(70, 3);
    assert!(leaving.replaces(None));
    assert!(!leaving.replaces(Some(&leaving)));
    assert!(leaving.replaces(Some(&kept(60, 2))));
    assert!(leaving.replaces(Some(&kept(60, 3))));
    assert!(!leaving.replaces(Some(&kept(80, 3))));
    assert!(!leaving.replaces(Some(&kept(60, 4))));
}
