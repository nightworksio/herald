use jiff::Timestamp;

use super::*;
use crate::announce::Stop;

#[test]
fn every_outcome_is_told_sent_or_stopped_or_as_it_was_where_nothing_was_sent() {
    let sealed_as = |stopped| Outcome::Sealed { stopped };
    let told = [
        (Outcome::Sent { steps: Vec::new() }, sealed_as(false)),
        (Outcome::AlreadyAnnounced, sealed_as(false)),
        (sealed_as(true), sealed_as(true)),
        (sealed_as(false), sealed_as(false)),
        (
            crate::announce::stopped(Stop::Refused("no".to_owned())),
            sealed_as(true),
        ),
    ];
    for (outcome, expected) in told {
        assert_eq!(sealed(outcome.clone()), expected, "{outcome:?}");
    }
    let untouched = [
        Outcome::Unedited,
        Outcome::Skipped("no project".to_owned()),
        Outcome::Gathered {
            key: "k".to_owned(),
            due: Timestamp::UNIX_EPOCH,
        },
        Outcome::Held {
            by: "run".to_owned(),
            until: Timestamp::UNIX_EPOCH,
        },
    ];
    for outcome in untouched {
        assert_eq!(sealed(outcome.clone()), outcome);
    }
}
