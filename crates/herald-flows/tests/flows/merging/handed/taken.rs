//! `herald merge-queue --apply` takes a pull request it handed to the
//! forge's queue out of it again once a cancel made after the gesture it
//! handed over stands, once per cancel, with the queue's own write
//! credential alone, after reading it again right before the call and
//! recording the step first.

use herald_core::merging::{CancelKey, TakeOut, TakenOut};
use herald_flows::{
    Cancels, RACED_OUT, RECANCELLED, TookOut, UNHANDED, UNNAMED_CANCEL, UNREAD_CANCEL,
    UNROLED_CANCEL, UNTAKEN, Undo,
};

use super::*;

const MERGER: &str = "GET https://api.github.com/repos/o/r/collaborators/m/permission";

const DEQUEUE: &str = "dequeuePullRequest";

fn mark(event: &str, id: Option<u64>, by: Option<&str>, at: &str) -> Value {
    json!({ "id": id, "event": event, "actor": by.map(|by| json!({ "login": by, "id": 3 })),
        "created_at": format!("2026-11-01T{at}:00Z"), "label": { "name": "herald:merge" } })
}

fn unlabeled() -> Value {
    mark("unlabeled", Some(502), Some("m"), "11:30")
}

fn events(marks: &[Value]) -> Response {
    answer(200, &json!(marks))
}

fn in_queue(total: usize) -> Response {
    answer(
        200,
        &json!({ "data": { "repository": { "mergeQueue": { "entries": {
            "totalCount": total, "nodes": [{ "pullRequest": { "number": 7 } }]
        } } } } }),
    )
}

fn dequeued() -> Response {
    answer(
        200,
        &json!({ "data": { "dequeuePullRequest": { "mergeQueueEntry": { "id": "MQE_1" } } } }),
    )
}

/// The forge with pull request 7 in its merge queue, `w`'s label taken off
/// again by `m`, a writer.
fn taken_back() -> Forge {
    taken_back_read(&pull("h7", &[]))
}

/// [`taken_back`], pull request 7 read again on its own as `fresh`.
fn taken_back_read(fresh: &Value) -> Forge {
    scripted()
        .answering(
            "GET https://api.github.com/repos/o/r/pulls?state=open&base=main&per_page=100&page=1",
            &[answer(200, &json!([pull("h7", &[])]))],
        )
        .answering(PULL, &[answer(200, fresh)])
        .answering(EVENTS, &[events(&[labeled(), unlabeled()])])
        .answering(MERGER, &[writer(true)])
        .answering(IN_QUEUE, &[in_queue(1)])
        .answering(GRAPHQL, &[dequeued()])
}

fn cancel() -> CancelKey {
    CancelKey {
        id: 502,
        by: "m".to_owned(),
        at: "2026-11-01T11:30:00Z".parse().unwrap(),
    }
}

fn read_with(
    store: &Failing,
    forge: &Forge,
    (extra, apply, budget): (&str, bool, Budget<'_>),
) -> Cancels {
    let read = run_read(
        &format!("{QUEUE}{extra}"),
        (store, forge, &the_secrets()),
        (apply, budget),
    );
    read.cancels
}

/// What the queue does with each pull request whose gesture was taken
/// back, applied where `apply`.
fn undone(store: &Failing, forge: &Forge, apply: bool) -> Vec<Undo> {
    match read_with(store, forge, ("", apply, Budget::UNBOUNDED)) {
        Cancels::Read(cancels) => cancels.into_iter().map(|cancel| cancel.next).collect(),
        unread @ Cancels::Unread(_) => panic!("{unread:?}"),
    }
}

fn taken_out(store: &Failing) -> Vec<TakeOut> {
    record(store).unwrap().taken_out
}

fn dequeues(forge: &Forge) -> usize {
    forge
        .bodies(GRAPHQL)
        .iter()
        .filter(|body| {
            body["query"]
                .as_str()
                .is_some_and(|query| query.contains(DEQUEUE))
        })
        .count()
}

#[test]
fn a_gesture_taken_back_after_the_hand_off_is_taken_out_of_the_forge_s_queue_once() {
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let forge = taken_back();
    assert_eq!(undone(&store, &forge, false), [Undo::TakeOut]);
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    assert_eq!(taken_out(&store), []);
    assert_eq!(undone(&store, &forge, true), [Undo::Took(TookOut::Out)]);
    assert_eq!(forge.carrying("write-value"), [GRAPHQL]);
    assert!(
        forge
            .carrying("read-value")
            .iter()
            .all(|line| line.starts_with("GET ") || line == IN_QUEUE)
    );
    assert_eq!(dequeues(&forge), 1);
    assert_eq!(
        forge.bodies(GRAPHQL).last().unwrap()["variables"],
        json!({ "id": "PR_7" })
    );
    let lines = forge.lines();
    let call = lines.iter().rposition(|line| line == GRAPHQL).unwrap();
    let fresh = lines[..call].iter().rposition(|line| line == PULL).unwrap();
    let queue = lines.iter().position(|line| line == IN_QUEUE).unwrap();
    assert!(queue < fresh, "{lines:?}");
    assert!(lines[fresh..call].contains(&EVENTS.to_owned()), "{lines:?}");
    assert!(lines[fresh..call].contains(&MERGER.to_owned()), "{lines:?}");
    assert_eq!(
        taken_out(&store),
        [TakeOut {
            cancel: Some(cancel()),
            credential: "HERALD_MERGE".to_owned(),
            at: NOON,
            outcome: TakenOut::Out,
        }]
    );
    assert_eq!(
        record(&store).unwrap().hand_off.unwrap().outcome,
        HandedOff::Queued
    );
    let again = taken_back();
    for apply in [false, true] {
        assert_eq!(
            undone(&store, &again, apply),
            [Undo::Recorded {
                outcome: TakenOut::Out,
                at: NOON
            }]
        );
    }
    assert_eq!(again.carrying("write-value"), Vec::<String>::new());
}

#[test]
fn a_pull_request_found_already_queued_is_recorded_with_no_call_and_a_cancel_takes_it_out() {
    let store = healthy();
    let forge = scripted().answering(QUEUED, &[queued(true)]);
    assert_eq!(
        run("", (&store, &forge, &the_secrets()), true),
        [Next::Took(Took::FoundQueued)]
    );
    assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    let found = record(&store).unwrap().hand_off.unwrap();
    assert_eq!(
        (found.head.as_str(), found.outcome),
        ("h7", HandedOff::FoundQueued)
    );
    let again = scripted().answering(QUEUED, &[queued(true)]);
    assert_eq!(
        run("", (&store, &again, &the_secrets()), true),
        [Next::Handed {
            outcome: HandedOff::FoundQueued,
            at: NOON
        }]
    );
    assert!(
        !again
            .lines()
            .iter()
            .any(|line| line == GRAPHQL || line == QUEUED)
    );
    let cancelled = taken_back();
    assert_eq!(undone(&store, &cancelled, true), [Undo::Took(TookOut::Out)]);
    assert_eq!(dequeues(&cancelled), 1);
    assert_eq!(taken_out(&store)[0].outcome, TakenOut::Out);
}

#[test]
fn a_cancel_by_one_without_the_role_or_without_an_id_is_shown_and_takes_nothing_out() {
    let unroled = taken_back().answering(MERGER, &[writer(false)]);
    let deleted = taken_back().answering(
        EVENTS,
        &[events(&[
            labeled(),
            mark("unlabeled", Some(502), None, "11:30"),
        ])],
    );
    let unnamed = taken_back().answering(
        EVENTS,
        &[events(&[
            labeled(),
            mark("unlabeled", None, Some("m"), "11:30"),
        ])],
    );
    for (forge, why) in [
        (unroled, UNROLED_CANCEL),
        (deleted, UNROLED_CANCEL),
        (unnamed, UNNAMED_CANCEL),
    ] {
        let store = healthy();
        kept(&store, HandedOff::Queued);
        for apply in [false, true] {
            assert_eq!(
                undone(&store, &forge, apply),
                [Undo::Uncarried(why.to_owned())]
            );
        }
        assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
        assert_eq!(dequeues(&forge), 0);
        assert_eq!(taken_out(&store), []);
        assert!(
            !forge
                .lines()
                .iter()
                .any(|line| line.ends_with("/collaborators//permission"))
        );
    }
}

#[test]
fn a_pull_request_herald_never_handed_over_is_left_in_the_forge_s_queue() {
    let forge = taken_back();
    let store = healthy();
    assert_eq!(undone(&store, &forge, true), []);
    for outcome in [HandedOff::Refused, HandedOff::HeadMoved, HandedOff::Unsent] {
        let store = healthy();
        kept(&store, outcome);
        assert_eq!(undone(&store, &forge, true), []);
        assert_eq!(taken_out(&store), []);
    }
    let store = healthy();
    kept_seen(&store, "h7");
    assert_eq!(undone(&store, &forge, true), []);
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    let unqueued = taken_back().answering(
        IN_QUEUE,
        &[answer(
            200,
            &json!({ "data": { "repository": { "mergeQueue": null } } }),
        )],
    );
    let store = healthy();
    kept(&store, HandedOff::Queued);
    assert_eq!(undone(&store, &unqueued, true), []);
    let gestured = taken_back().answering(
        EVENTS,
        &[events(&[
            labeled(),
            unlabeled(),
            mark("labeled", Some(503), Some("w"), "11:45"),
        ])],
    );
    assert_eq!(undone(&store, &gestured, true), []);
    assert_eq!(gestured.carrying("write-value"), Vec::<String>::new());
}

#[test]
fn what_decides_a_cancel_read_in_part_or_not_at_all_takes_nothing_out() {
    let failing = [
        taken_back().answering(IN_QUEUE, &[answer(500, &json!({}))]),
        taken_back().answering(IN_QUEUE, &[in_queue(2)]),
        taken_back().answering(EVENTS, &[answer(500, &json!({}))]),
    ];
    for forge in failing {
        let store = healthy();
        kept(&store, HandedOff::Queued);
        for apply in [false, true] {
            let cancels = read_with(&store, &forge, ("", apply, Budget::UNBOUNDED));
            assert!(matches!(cancels, Cancels::Unread(Some(_))), "{cancels:?}");
        }
        assert_eq!(dequeues(&forge), 0);
    }
    let mut capped = taken_back();
    for page in 1..=5 {
        let mut marks: Vec<Value> = (0..100)
            .map(|at| {
                json!({ "id": 9000 + at, "event": "labeled", "actor": { "login": "x", "id": 9 },
                    "created_at": "2026-11-01T09:00:00Z", "label": { "name": "other" } })
            })
            .collect();
        if page == 1 {
            marks.splice(0..2, [labeled(), unlabeled()]);
        }
        capped = capped.answering(
            &format!(
                "GET https://api.github.com/repos/o/r/issues/7/events?per_page=100&page={page}"
            ),
            &[events(&marks)],
        );
    }
    let store = healthy();
    kept(&store, HandedOff::Queued);
    assert_eq!(
        undone(&store, &capped, true),
        [Undo::Uncarried(UNREAD_CANCEL.to_owned())]
    );
    assert_eq!(dequeues(&capped), 0);
    let unread = Failing {
        fault: Fault::ReadsActions,
        ..healthy()
    };
    kept(&unread, HandedOff::Queued);
    let forge = taken_back();
    let shown = undone(&unread, &forge, true);
    assert!(
        matches!(&shown[..], [Undo::Uncarried(why)] if why.starts_with("herald's record of the gesture could not be read")),
        "{shown:?}"
    );
    assert_eq!(dequeues(&forge), 0);
}

#[test]
fn what_changed_since_the_queue_s_read_takes_nothing_out() {
    let first = events(&[labeled(), unlabeled()]);
    let changed = [
        (
            taken_back().answering(MERGER, &[writer(true), writer(false)]),
            UNROLED_CANCEL,
        ),
        (
            taken_back().answering(
                EVENTS,
                &[
                    first.clone(),
                    events(&[
                        labeled(),
                        unlabeled(),
                        mark("labeled", Some(503), Some("w"), "11:45"),
                    ]),
                ],
            ),
            UNTAKEN,
        ),
        (
            taken_back().answering(
                EVENTS,
                &[
                    first.clone(),
                    events(&[
                        labeled(),
                        unlabeled(),
                        mark("labeled", Some(503), Some("w"), "11:45"),
                        mark("unlabeled", Some(504), Some("m"), "11:50"),
                    ]),
                ],
            ),
            RECANCELLED,
        ),
        (
            taken_back().answering(
                EVENTS,
                &[
                    first.clone(),
                    events(&[label("labeled", 505, "11:00"), unlabeled()]),
                ],
            ),
            UNHANDED,
        ),
        (
            taken_back().answering(
                PULL,
                &[answer(200, &pull("h7", &[])), answer(404, &json!({}))],
            ),
            UNTAKEN,
        ),
    ];
    for (forge, why) in changed {
        let store = healthy();
        kept(&store, HandedOff::Queued);
        assert_eq!(
            undone(&store, &forge, true),
            [Undo::Uncarried(why.to_owned())]
        );
        assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
        assert_eq!(taken_out(&store), []);
    }
    let unread = taken_back().answering(
        PULL,
        &[answer(200, &pull("h7", &[])), answer(500, &json!({}))],
    );
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let shown = undone(&store, &unread, true);
    assert!(
        matches!(&shown[..], [Undo::Uncarried(why)] if why.starts_with("it could not be read again right before the step")),
        "{shown:?}"
    );
    assert_eq!(dequeues(&unread), 0);
}

#[test]
fn the_record_is_written_before_the_call_and_what_came_of_it_after() {
    for (fault, undo) in [
        (
            Fault::ActionsConflict,
            Undo::Uncarried(RACED_OUT.to_owned()),
        ),
        (
            Fault::SettlesNoAction,
            Undo::Took(TookOut::Unrecorded(TakenOut::Out)),
        ),
    ] {
        let sent = usize::from(fault == Fault::SettlesNoAction);
        let store = Failing { fault, ..healthy() };
        kept(&store, HandedOff::Queued);
        let forge = taken_back();
        assert_eq!(undone(&store, &forge, true), [undo]);
        assert_eq!(dequeues(&forge), sent);
    }
    let unwritten = Failing {
        fault: Fault::WritesActions,
        ..healthy()
    };
    kept(&unwritten, HandedOff::Queued);
    let forge = taken_back();
    let shown = undone(&unwritten, &forge, true);
    assert!(
        matches!(&shown[..], [Undo::Uncarried(why)] if why.starts_with("herald's record of it could not be written")),
        "{shown:?}"
    );
    assert_eq!(dequeues(&forge), 0);
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let spent = taken_back();
    let budget = Budget {
        until: Some(NOON),
        ..Budget::UNBOUNDED
    };
    let Cancels::Read(unsent) = read_with(&store, &spent, ("", true, budget)) else {
        panic!("unread");
    };
    assert_eq!(
        unsent[0].next,
        Undo::Uncarried(TakenOut::Unsent.said().to_owned())
    );
    assert_eq!(dequeues(&spent), 0);
    assert_eq!(taken_out(&store)[0].outcome, TakenOut::Unsent);
    assert_eq!(
        undone(&store, &taken_back(), true),
        [Undo::Took(TookOut::Out)]
    );
    assert_eq!(taken_out(&store).len(), 1);
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let mut record = record(&store).unwrap();
    record.taken_out.push(TakeOut {
        cancel: Some(cancel()),
        credential: "HERALD_MERGE".to_owned(),
        at: NOON,
        outcome: TakenOut::Calling,
    });
    let at = pollster::block_on(store.inner.gesture(&key()))
        .unwrap()
        .unwrap()
        .version;
    pollster::block_on(store.inner.put_gesture(&record, Some(at))).unwrap();
    let forge = taken_back();
    assert_eq!(
        undone(&store, &forge, true),
        [Undo::Recorded {
            outcome: TakenOut::Calling,
            at: NOON
        }]
    );
    assert_eq!(dequeues(&forge), 0);
}

#[test]
fn a_lost_or_refused_answer_is_read_back_and_a_new_cancel_asks_again() {
    let lost = answer(502, &json!({}));
    let refused = answer(
        200,
        &json!({ "errors": [{ "message": "not queued, write-value" }] }),
    );
    for ((answer, back), undo, outcome) in [
        (
            (lost.clone(), queued(true)),
            Undo::Took(TookOut::Unsettled),
            TakenOut::Unsettled,
        ),
        (
            (lost, queued(false)),
            Undo::Took(TookOut::Out),
            TakenOut::Out,
        ),
        (
            (refused.clone(), queued(true)),
            Undo::Took(TookOut::Declined(
                "GitHub refused it: not queued, [the write credential]".to_owned(),
            )),
            TakenOut::Refused,
        ),
        (
            (refused, queued(false)),
            Undo::Took(TookOut::Out),
            TakenOut::Out,
        ),
    ] {
        let store = healthy();
        kept(&store, HandedOff::Queued);
        let forge = taken_back()
            .answering(GRAPHQL, &[answer])
            .answering(QUEUED, &[back]);
        assert_eq!(undone(&store, &forge, true), [undo]);
        assert_eq!(dequeues(&forge), 1);
        assert_eq!(forge.carrying("read-value").last().unwrap(), QUEUED);
        assert_eq!(taken_out(&store)[0].outcome, outcome);
        let again = taken_back();
        assert_eq!(
            undone(&store, &again, true),
            [Undo::Recorded { outcome, at: NOON }]
        );
        assert_eq!(dequeues(&again), 0);
        let recancelled = taken_back().answering(
            EVENTS,
            &[events(&[
                labeled(),
                unlabeled(),
                mark("labeled", Some(503), Some("w"), "11:45"),
                mark("unlabeled", Some(504), Some("m"), "11:50"),
            ])],
        );
        assert_eq!(
            undone(&store, &recancelled, true),
            [Undo::Took(TookOut::Out)]
        );
        assert_eq!(dequeues(&recancelled), 1);
        let taken = taken_out(&store);
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[1].cancel.as_ref().map(|cancel| cancel.id), Some(504));
    }
}

#[test]
fn a_gesture_made_again_after_a_cancel_is_another_record_and_is_handed_over_again() {
    let store = healthy();
    kept(&store, HandedOff::Queued);
    assert_eq!(
        undone(&store, &taken_back(), true),
        [Undo::Took(TookOut::Out)]
    );
    let before = record(&store).unwrap();
    let regestured = taken_back()
        .answering(
            "GET https://api.github.com/repos/o/r/pulls?state=open&base=main&per_page=100&page=1",
            &[answer(200, &json!([pull("h7", &["herald:merge"])]))],
        )
        .answering(PULL, &[answer(200, &pull("h7", &["herald:merge"]))])
        .answering(
            EVENTS,
            &[events(&[labeled(), unlabeled(), mark("labeled", Some(503), Some("w"), "11:45")])],
        )
        .answering(IN_QUEUE, &[answer(200, &json!({ "data": { "repository": { "mergeQueue": { "entries": { "totalCount": 0, "nodes": [] } } } } }))])
        .answering(GRAPHQL, &[enqueued()]);
    assert_eq!(
        run("", (&store, &regestured, &the_secrets()), true),
        [Next::Took(Took::Queued)]
    );
    let again = GestureKey {
        gesture: 503,
        ..key()
    };
    let handed = pollster::block_on(store.inner.gesture(&again))
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(handed.hand_off.unwrap().outcome, HandedOff::Queued);
    assert_eq!(handed.taken_out, []);
    assert_eq!(record(&store).unwrap(), before);
}

#[test]
fn a_queue_that_takes_no_step_takes_nothing_out() {
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let forge = taken_back();
    for apply in [false, true] {
        let cancels = read_with(
            &store,
            &forge,
            ("applied_by = \"server\"\n", apply, Budget::UNBOUNDED),
        );
        let Cancels::Read(cancels) = cancels else {
            panic!("unread");
        };
        assert_eq!(cancels[0].next, Undo::Idle);
    }
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    assert_eq!(taken_out(&store), []);
}

#[test]
fn a_take_out_that_cannot_be_made_or_whose_app_token_was_never_minted_is_recorded_unsent() {
    let mut nameless = pull("h7", &[]);
    nameless["node_id"] = Value::Null;
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let forge = taken_back_read(&nameless);
    let unsent = undone(&store, &forge, true);
    assert!(
        matches!(&unsent[..], [Undo::Uncarried(why)] if why.contains("taken out of the merge queue")),
        "{unsent:?}"
    );
    assert_eq!(taken_out(&store)[0].outcome, TakenOut::Unsent);
    assert_eq!(dequeues(&forge), 0);
    let app = "[github_apps.merger]\nclient_id = \"Iv23liMerger\"\nprivate_key = { secret = \"HERALD_MERGER_KEY\" }\n";
    let as_app = format!("{QUEUE}{app}").replace(
        "write = { token = { secret = \"HERALD_MERGE\" } }",
        "write = { app = \"merger\" }",
    );
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let forge = taken_back();
    let held = the_secrets().with("HERALD_MERGER_KEY", "the App's key");
    let read = run_read(&as_app, (&store, &forge, &held), (true, Budget::UNBOUNDED));
    let Cancels::Read(cancels) = read.cancels else {
        panic!("unread");
    };
    assert!(
        matches!(&cancels[0].next, Undo::Uncarried(why) if why.starts_with("the write App's token could not be minted")),
        "{cancels:?}"
    );
    assert_eq!(taken_out(&store)[0].outcome, TakenOut::Unsent);
    assert_eq!(dequeues(&forge), 0);
}

#[test]
fn a_take_out_without_its_write_credential_says_why_and_writes_nothing() {
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let forge = taken_back();
    let held = secrets().with("HERALD_READ", "read-value");
    let read = run_read(QUEUE, (&store, &forge, &held), (true, Budget::UNBOUNDED));
    let Cancels::Read(cancels) = read.cancels else {
        panic!("unread");
    };
    assert!(
        matches!(&cancels[0].next, Undo::Uncarried(why) if !why.is_empty()),
        "{cancels:?}"
    );
    assert_eq!(taken_out(&store), []);
    assert_eq!(dequeues(&forge), 0);
}

mod lost;
