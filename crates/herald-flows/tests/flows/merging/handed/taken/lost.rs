use herald_flows::{Cancel, UNMATCHED, UNMATCHED_OUT, UNSAID_ROLE};

use super::*;

const AUTHOR: &str = "GET https://api.github.com/repos/o/r/collaborators/r/permission";

/// [`taken_back`] with `marks` on pull request 7 and `w` answered `role`
/// in turn.
fn lost(marks: &[Value], role: &[Response]) -> Forge {
    taken_back()
        .answering(EVENTS, &[events(marks)])
        .answering(PERMISSION, role)
}

/// A mark of the label by `w` with the forge's id `id` at `at`, given to
/// the second.
fn gesture(event: &str, id: u64, at: &str) -> Value {
    json!({ "id": id, "event": event, "actor": { "login": "w", "id": 2 },
        "created_at": format!("2026-11-01T{at}Z"), "label": { "name": "herald:merge" } })
}

fn read(store: &Failing, forge: &Forge, extra: &str) -> Vec<Cancel> {
    match read_with(store, forge, (extra, true, Budget::UNBOUNDED)) {
        Cancels::Read(cancels) => cancels,
        unread @ Cancels::Unread(_) => panic!("{unread:?}"),
    }
}

#[test]
fn a_gesture_s_maker_who_lost_the_role_has_it_taken_out_whatever_cancel_stands() {
    for (marks, merger, cancel) in [
        (vec![labeled()], true, None),
        (vec![labeled(), unlabeled()], false, None),
        (vec![labeled(), unlabeled()], true, Some(cancel())),
    ] {
        let store = healthy();
        kept(&store, HandedOff::Queued);
        let forge = lost(&marks, &[writer(false)]).answering(MERGER, &[writer(merger)]);
        assert_eq!(undone(&store, &forge, false), [Undo::TakeOut], "{marks:?}");
        assert_eq!(undone(&store, &forge, true), [Undo::Took(TookOut::Out)]);
        assert_eq!(dequeues(&forge), 1);
        assert_eq!(taken_out(&store)[0].cancel, cancel);
    }
}

#[test]
fn a_maker_who_holds_the_role_keeps_it_in_and_a_cancel_without_the_role_is_shown() {
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let held = lost(&[labeled()], &[writer(true)]);
    assert_eq!(undone(&store, &held, true), []);
    let low = lost(&[labeled(), unlabeled()], &[writer(true)]).answering(MERGER, &[writer(false)]);
    assert_eq!(
        undone(&store, &low, true),
        [Undo::Uncarried(UNROLED_CANCEL.to_owned())]
    );
    let regained = lost(&[labeled()], &[writer(false), writer(true)]);
    assert_eq!(
        undone(&store, &regained, true),
        [Undo::Uncarried(UNTAKEN.to_owned())]
    );
    for forge in [&held, &low, &regained] {
        assert_eq!(dequeues(forge), 0);
    }
    assert_eq!(taken_out(&store), []);
}

#[test]
fn a_queue_that_says_stay_leaves_it_in_when_the_role_is_lost() {
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let forge = lost(&[labeled()], &[writer(false)]);
    let read = read(&store, &forge, "role_lost = \"stay\"\n");
    assert_eq!(
        read.iter().map(|cancel| &cancel.next).collect::<Vec<_>>(),
        [&Undo::Stays]
    );
    assert_eq!(dequeues(&forge), 0);
    assert_eq!(taken_out(&store), []);
}

#[test]
fn a_maker_whose_account_is_gone_holds_no_role_and_is_named_as_recorded() {
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let ghost = json!({ "id": 501, "event": "labeled", "actor": { "login": "ghost", "id": 10137 },
        "created_at": "2026-11-01T11:00:00Z", "label": { "name": "herald:merge" } });
    let forge = lost(&[ghost], &[writer(true)]);
    let read = read(&store, &forge, "");
    assert_eq!(read[0].next, Undo::Took(TookOut::Out));
    assert_eq!(read[0].renamed.as_deref(), Some("w"));
    assert_eq!(dequeues(&forge), 1);
    assert_eq!(taken_out(&store)[0].cancel, None);
}

#[test]
fn a_forge_that_will_not_say_the_maker_s_role_takes_nothing_out() {
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let forge = lost(&[labeled()], &[answer(200, &json!({}))]);
    assert_eq!(
        undone(&store, &forge, true),
        [Undo::Uncarried(UNSAID_ROLE.to_owned())]
    );
    assert_eq!(dequeues(&forge), 0);
}

#[test]
fn the_same_gesture_id_at_another_time_is_another_gesture_and_writes_nothing() {
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let before = record(&store);
    let other = gesture("labeled", 501, "11:00:01");
    let forge = lost(std::slice::from_ref(&other), &[writer(false)]);
    assert_eq!(
        undone(&store, &forge, true),
        [Undo::Uncarried(UNMATCHED_OUT.to_owned())]
    );
    assert_eq!(dequeues(&forge), 0);
    let store = healthy();
    kept(&store, HandedOff::Unsent);
    let before_entry = record(&store);
    let entered = scripted().answering(EVENTS, &[events(&[other])]);
    assert_eq!(
        run("", (&store, &entered, &the_secrets()), true),
        [Next::Took(Took::Refused(UNMATCHED.to_owned()))]
    );
    assert!(!entered.lines().contains(&GRAPHQL.to_owned()));
    assert_eq!(record(&store), before_entry);
    assert!(before.is_some());
}

#[test]
fn two_gestures_a_second_apart_are_two_records() {
    let store = healthy();
    let first = scripted().answering(EVENTS, &[events(&[gesture("labeled", 501, "11:00:00")])]);
    assert_eq!(
        run("", (&store, &first, &the_secrets()), true),
        [Next::Took(Took::Queued)]
    );
    let marks = [
        gesture("labeled", 501, "11:00:00"),
        gesture("unlabeled", 502, "11:00:00"),
        gesture("labeled", 503, "11:00:01"),
    ];
    let second = scripted()
        .answering(EVENTS, &[events(&marks)])
        .answering(IN_QUEUE, &[in_queue(0)]);
    assert_eq!(
        run("", (&store, &second, &the_secrets()), true),
        [Next::Took(Took::Queued)]
    );
    for (id, at) in [(501, "2026-11-01T11:00:00Z"), (503, "2026-11-01T11:00:01Z")] {
        let key = GestureKey {
            gesture: id,
            ..key()
        };
        let kept = pollster::block_on(store.inner.gesture(&key))
            .unwrap()
            .unwrap()
            .value;
        assert_eq!(kept.at, at.parse().unwrap(), "{id}");
        assert_eq!(kept.hand_off.unwrap().outcome, HandedOff::Queued);
    }
}

#[test]
fn the_same_gesture_id_by_another_maker_is_another_gesture_and_writes_nothing() {
    let store = healthy();
    kept(&store, HandedOff::Unsent);
    let read = pollster::block_on(store.inner.gesture(&key()))
        .unwrap()
        .unwrap();
    let mut renamed = read.value;
    renamed.by = "v".to_owned();
    pollster::block_on(store.inner.put_gesture(&renamed, Some(read.version))).unwrap();
    let before = record(&store);
    let forge = scripted();
    assert_eq!(
        run("", (&store, &forge, &the_secrets()), true),
        [Next::Took(Took::Refused(UNMATCHED.to_owned()))]
    );
    assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
    assert_eq!(record(&store), before);
}

#[test]
fn a_later_gesture_the_forge_named_no_id_for_is_passed_over_for_the_one_herald_handed() {
    let store = healthy();
    kept(&store, HandedOff::Queued);
    let unnamed = mark("labeled", None, Some("w"), "11:10");
    let forge = lost(&[labeled(), unnamed, unlabeled()], &[writer(true)]);
    assert_eq!(undone(&store, &forge, true), [Undo::Took(TookOut::Out)]);
    assert_eq!(dequeues(&forge), 1);
    assert_eq!(taken_out(&store)[0].cancel, Some(cancel()));
}

#[test]
fn the_take_out_is_judged_against_the_gesture_herald_handed_over() {
    let author = |event: &str, id: u64, at: &str| mark(event, Some(id), Some("r"), at);
    let earlier = mark("unlabeled", Some(500), Some("m"), "10:30");
    let regestured = mark("labeled", Some(503), Some("m"), "11:45");
    let out = vec![Undo::Took(TookOut::Out)];
    for (marks, held, next, cancel) in [
        (
            vec![labeled(), unlabeled(), author("unlabeled", 505, "11:40")],
            true,
            out.clone(),
            Some(cancel()),
        ),
        (
            vec![labeled(), author("unlabeled", 505, "11:20"), unlabeled()],
            true,
            out.clone(),
            Some(cancel()),
        ),
        (
            vec![labeled(), unlabeled(), author("labeled", 503, "11:45")],
            true,
            out.clone(),
            Some(cancel()),
        ),
        (vec![earlier, labeled()], false, out, None),
        (
            vec![labeled(), author("unlabeled", 505, "11:40")],
            true,
            vec![Undo::Uncarried(UNROLED_CANCEL.to_owned())],
            None,
        ),
        (vec![labeled(), regestured], false, Vec::new(), None),
    ] {
        let store = healthy();
        kept(&store, HandedOff::Queued);
        let forge = lost(&marks, &[writer(held)])
            .answering(MERGER, &[writer(true)])
            .answering(AUTHOR, &[writer(false)]);
        assert_eq!(undone(&store, &forge, true), next, "{marks:?}");
        let taken = taken_out(&store);
        assert_eq!(dequeues(&forge), taken.len(), "{marks:?}");
        assert_eq!(taken.first().and_then(|out| out.cancel.clone()), cancel);
        let again = lost(&marks, &[writer(held)])
            .answering(MERGER, &[writer(true)])
            .answering(AUTHOR, &[writer(false)]);
        let once = undone(&store, &again, true);
        assert!(
            once.iter().all(|next| !matches!(next, Undo::Took(_))),
            "{once:?}"
        );
        assert_eq!(dequeues(&again), 0);
    }
}

#[test]
fn a_pull_request_shown_beside_a_step_keeps_what_its_read_said() {
    let store = healthy();
    let other = healthy();
    kept(&other, HandedOff::Queued);
    let mut eight = record(&other).unwrap();
    eight.key.number = 8;
    eight.key.gesture = 801;
    pollster::block_on(store.inner.put_gesture(&eight, None)).unwrap();
    let mut pull_eight = pull("h8", &[]);
    pull_eight["number"] = json!(8);
    pull_eight["node_id"] = json!("PR_8");
    let queue = json!({ "data": { "repository": { "mergeQueue": { "entries": {
        "totalCount": 1, "nodes": [{ "pullRequest": { "number": 8 } }]
    } } } } });
    let forge = scripted()
        .answering(IN_QUEUE, &[answer(200, &queue)])
        .answering(
            "GET https://api.github.com/repos/o/r/pulls/8",
            &[answer(200, &pull_eight)],
        )
        .answering(
            "GET https://api.github.com/repos/o/r/issues/8/events?per_page=100&page=1",
            &[events(&[
                gesture("labeled", 801, "11:00:00"),
                mark("unlabeled", Some(802), Some("r"), "11:30"),
            ])],
        )
        .answering(AUTHOR, &[writer(false)]);
    let read = read(&store, &forge, "");
    assert_eq!(
        read.iter().map(|cancel| &cancel.next).collect::<Vec<_>>(),
        [&Undo::Uncarried(UNROLED_CANCEL.to_owned())]
    );
    assert_eq!(dequeues(&forge), 0);
    assert_eq!(
        record(&store).unwrap().hand_off.unwrap().outcome,
        HandedOff::Queued
    );
}
