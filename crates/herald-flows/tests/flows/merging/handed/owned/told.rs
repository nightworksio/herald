//! herald's own queue keeps the last time each pull request left it in a
//! record of the pull request's own, which a view's row reads, and writes
//! it again on the next run where that write failed.

use herald_core::Dequeued;
use herald_core::merging::{Left, LeftRecord, StepKind, Stepped, Verdict};
use herald_flows::{Acted, Next};
use herald_ports::{Actions as _, Versioned};
use serde_json::json;

use super::{MERGE, PULL, after, answer, applied, github, key, left, open, reads, record};
use crate::harness::{Failing, Fault, healthy};

fn kept(store: &Failing) -> Option<Versioned<LeftRecord>> {
    pollster::block_on(store.inner.left_record(&key().pull())).unwrap()
}

fn refusing() -> super::Forge {
    let refused = answer(405, &json!({ "message": "Base branch was modified" }));
    github("h7", &[], 0).answering(MERGE, &[refused])
}

#[test]
fn a_pull_request_leaving_is_kept_for_its_rows_when_and_why_once() {
    let forge = refusing();
    let store = healthy();
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Stepped(
            StepKind::Merge,
            Stepped::Refused
        ))]
    );
    let leaving = record(&store).and_then(|record| record.own.left).unwrap();
    let first = kept(&store).unwrap();
    assert_eq!(
        first.value,
        LeftRecord {
            key: key().pull(),
            gesture: key().gesture,
            left: herald_core::Left {
                at: leaving.at,
                reason: Dequeued::MergeRefused,
            },
        }
    );
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Verdict(Verdict::Leaves(Left::MergeRefused))]
    );
    assert_eq!(kept(&store), Some(first));
}

#[test]
fn a_leaving_its_rows_could_not_be_told_of_is_said_and_written_by_the_next_run() {
    let forge = refusing();
    let mut store = healthy();
    store.fault = Fault::WritesLeft;
    for _ in 0..2 {
        assert_eq!(
            applied("", (&store, &forge), true),
            [Next::Acted(Acted::Untold(Left::MergeRefused))]
        );
        assert_eq!(left(&store), Some(Left::MergeRefused));
        assert_eq!(kept(&store), None);
    }
    store.fault = Fault::Nothing;
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Verdict(Verdict::Leaves(Left::MergeRefused))]
    );
    assert_eq!(
        kept(&store).map(|kept| kept.value.left.reason),
        Some(Dequeued::MergeRefused)
    );
}

#[test]
fn a_head_moved_since_the_queue_s_read_is_kept_for_its_rows_or_said_untold() {
    let probe = github("h7", &[], 0);
    applied("", (&healthy(), &probe), false);
    let pulls = reads(&probe, PULL);
    for (fault, acted) in [
        (Fault::Nothing, Acted::Left(Left::HeadMoved)),
        (Fault::WritesLeft, Acted::Untold(Left::HeadMoved)),
        (Fault::ReadsLeft, Acted::Untold(Left::HeadMoved)),
    ] {
        let forge = after(github("h7", &[], 0), PULL, (open("h7"), pulls), open("h9"));
        let mut store = healthy();
        store.fault = fault;
        assert_eq!(
            applied("", (&store, &forge), true),
            [Next::Acted(acted.clone())],
            "{fault:?}"
        );
        let reason = kept(&store).map(|kept| kept.value.left.reason);
        let told = (fault == Fault::Nothing).then_some(Dequeued::HeadMoved);
        assert_eq!(reason, told, "{fault:?}");
    }
}
