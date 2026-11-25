use std::collections::BTreeMap;

use herald_core::Permission;
use herald_core::merging::Runs;

use super::super::tests::{now, queue, taken_back};
use super::*;

/// The record of gesture 10 on pull request 1, made by `w` at `taken`'s
/// time for it.
fn kept(taken: &TakenBack) -> GestureRecord {
    GestureRecord {
        key: GestureKey {
            api: "https://api.github.com".to_owned(),
            repository: "o/r".to_owned(),
            number: 1,
            gesture: 10,
        },
        by: "w".to_owned(),
        at: taken.gestures[0].at,
        queue: "app-main".to_owned(),
        seen: "h1".to_owned(),
        seen_at: taken.gestures[0].at,
        hand_off: None,
        taken_out: Vec::new(),
        own: herald_core::merging::Own::default(),
    }
}

/// The queue judged with: least role maintain, skew 5 s.
fn asked() -> MergeQueue {
    let mut asked = queue(Runs::Forge);
    asked.least = Permission::Maintain;
    asked.skew = herald_core::merging::SKEW;
    asked
}

/// A mark with id `id` by `by`, `secs` seconds after minute zero.
fn mark(id: u64, by: &str, secs: i64) -> Marked {
    Marked {
        id: Some(id),
        by: by.to_owned(),
        at: now() + SignedDuration::from_secs(secs),
    }
}

/// A pull request with these marks, newest first, read whole: `h` holds
/// maintain, `a` admin, `w` write, `r` (its author) nothing, and the forge
/// would not say `u`'s role.
fn pull(gestures: Vec<Marked>, cancels: Vec<Marked>) -> TakenBack {
    TakenBack {
        gestures,
        cancels,
        makers: BTreeMap::from([
            ("h".to_owned(), Holds::Role(Permission::Maintain)),
            ("a".to_owned(), Holds::Role(Permission::Admin)),
            ("w".to_owned(), Holds::Role(Permission::Write)),
            ("r".to_owned(), Holds::Nothing),
            ("u".to_owned(), Holds::Unsaid),
        ]),
        ..taken_back(None)
    }
}

/// The key of `cancel`.
fn key(cancel: &Marked) -> CancelKey {
    CancelKey {
        id: cancel.id.unwrap(),
        by: cancel.by.clone(),
        at: cancel.at,
    }
}

/// `taken` judged by `asked()` against the gesture at `anchor`, the cancel
/// it names cloned.
fn judged(taken: &TakenBack, anchor: usize) -> (Carried, Option<Marked>) {
    let (verdict, shown) = carried(&asked(), taken, at(taken, anchor));
    (verdict, shown.cloned())
}

/// `taken`'s gesture at `anchor`, with those newer than it.
fn at(taken: &TakenBack, anchor: usize) -> Anchor<'_> {
    anchored(taken, (anchor, &taken.gestures[anchor]))
}

/// How many gestures are newer than the one `record` is of, where `taken`
/// holds it.
fn newer(taken: &TakenBack, record: &GestureRecord) -> Option<usize> {
    made(taken, record).map(|(newer, _)| newer.len())
}

#[test]
fn the_newest_cancel_after_the_gesture_handed_over_by_one_holding_the_role_takes_it_out() {
    let handed = mark(10, "h", 0);
    let holder = mark(12, "h", 120);
    let author = mark(13, "r", 180);
    let out = (Carried::Out(Some(key(&holder))), Some(holder.clone()));
    let later = pull(vec![handed.clone()], vec![author.clone(), holder.clone()]);
    assert_eq!(
        judged(&later, 0),
        out,
        "an author's cancel after it hides nothing"
    );
    let earlier = pull(
        vec![handed.clone()],
        vec![holder.clone(), mark(11, "r", 60)],
    );
    assert_eq!(judged(&earlier, 0), out, "an author's cancel before it");
    let newest = mark(14, "a", 240);
    let two = pull(vec![handed.clone()], vec![newest.clone(), holder.clone()]);
    assert_eq!(
        judged(&two, 0),
        (Carried::Out(Some(key(&newest))), Some(newest)),
        "the newest of two"
    );
    let alone = pull(vec![handed.clone()], vec![author.clone()]);
    assert_eq!(
        judged(&alone, 0),
        (Carried::Kept(UNROLED_CANCEL), Some(author.clone())),
        "an author's cancel alone is shown, not carried out"
    );
    let low = pull(vec![handed.clone()], vec![mark(15, "w", 60), author]);
    assert_eq!(
        judged(&low, 0).0,
        Carried::Kept(UNROLED_CANCEL),
        "one below the least role"
    );
    let mut unnamed = holder.clone();
    unnamed.id = None;
    let nameless = pull(vec![handed.clone()], vec![unnamed.clone()]);
    assert_eq!(
        judged(&nameless, 0),
        (Carried::Kept(UNNAMED_CANCEL), Some(unnamed.clone())),
    );
    let below = pull(
        vec![handed.clone()],
        vec![mark(16, "r", 180), unnamed.clone()],
    );
    assert_eq!(
        judged(&below, 0),
        (Carried::Kept(UNNAMED_CANCEL), Some(unnamed)),
        "the role-holder's cancel is shown, not a newer one below the role"
    );
    let none = pull(vec![handed.clone()], Vec::new());
    assert_eq!(
        judged(&none, 0),
        (Carried::Stands, None),
        "nothing takes it out"
    );
    let mut partial = later;
    partial.whole = false;
    assert_eq!(judged(&partial, 0), (Carried::Kept(UNREAD_CANCEL), None));
}

#[test]
fn a_newer_gesture_by_one_holding_the_role_after_the_cancel_stands() {
    let handed = mark(10, "h", 0);
    let holder = mark(12, "h", 120);
    let low = pull(
        vec![mark(20, "w", 180), handed.clone()],
        vec![holder.clone()],
    );
    assert_eq!(
        judged(&low, 1),
        (Carried::Out(Some(key(&holder))), Some(holder.clone())),
        "a gesture by one below the least role does not stand"
    );
    let again = pull(
        vec![mark(20, "h", 180), handed.clone()],
        vec![holder.clone()],
    );
    assert_eq!(judged(&again, 1), (Carried::Stands, None));
    let before = pull(
        vec![mark(20, "a", 60), handed.clone()],
        vec![holder.clone()],
    );
    assert_eq!(
        judged(&before, 1).0,
        Carried::Out(Some(key(&holder))),
        "a gesture before the cancel does not stand"
    );
    let unsaid = pull(
        vec![mark(20, "u", 180), handed.clone()],
        vec![holder.clone()],
    );
    assert_eq!(judged(&unsaid, 1).0, Carried::Out(Some(key(&holder))));
    let ghost = pull(vec![mark(20, "", 180), handed], vec![holder.clone()]);
    assert_eq!(judged(&ghost, 1).0, Carried::Out(Some(key(&holder))));
}

#[test]
fn the_gesture_s_maker_who_lost_the_role_has_it_taken_out_unless_a_newer_gesture_stands() {
    let earlier = mark(9, "h", -60);
    let handed = mark(10, "w", 0);
    let lost = pull(vec![handed.clone()], vec![earlier.clone()]);
    assert_eq!(
        judged(&lost, 0),
        (Carried::Out(None), None),
        "a cancel before the gesture handed over does not count"
    );
    let mut stay = asked();
    stay.role_lost = RoleLost::Stay;
    assert_eq!(carried(&stay, &lost, at(&lost, 0)), (Carried::Stays, None));
    let author = pull(vec![handed.clone()], vec![mark(11, "r", 60)]);
    assert_eq!(judged(&author, 0), (Carried::Out(None), None));
    let mut unnamed = mark(11, "h", 60);
    unnamed.id = None;
    let nameless = pull(vec![handed.clone()], vec![unnamed]);
    assert_eq!(judged(&nameless, 0), (Carried::Out(None), None));
    let gone = pull(vec![mark(10, "", 0)], Vec::new());
    assert_eq!(
        judged(&gone, 0),
        (Carried::Out(None), None),
        "an account gone"
    );
    let holder = mark(12, "h", 120);
    let cancelled = pull(vec![handed.clone()], vec![holder.clone()]);
    let out = (Carried::Out(Some(key(&holder))), Some(holder));
    assert_eq!(judged(&cancelled, 0), out, "a cancel is named first");
    assert_eq!(
        carried(&stay, &cancelled, at(&cancelled, 0)),
        (out.0, out.1.as_ref())
    );
    let regestured = pull(vec![mark(20, "h", 180), handed.clone()], Vec::new());
    assert_eq!(judged(&regestured, 1), (Carried::Stands, None));
    let unsaid = pull(vec![mark(10, "u", 0)], Vec::new());
    assert_eq!(judged(&unsaid, 0), (Carried::Kept(UNSAID_ROLE), None));
    let shown = mark(11, "r", 60);
    let unsaid = pull(vec![mark(10, "u", 0)], vec![shown.clone()]);
    assert_eq!(
        judged(&unsaid, 0),
        (Carried::Kept(UNROLED_CANCEL), Some(shown))
    );
    let mut unread = lost;
    unread.makers.clear();
    assert_eq!(judged(&unread, 0), (Carried::Kept(UNSAID_ROLE), None));
}

#[test]
fn a_cancel_stamped_within_the_skew_before_a_gesture_counts_as_after_it() {
    let skew = herald_core::merging::SKEW.as_secs();
    let handed = mark(10, "w", 0);
    let within = mark(12, "h", -skew);
    let edge = pull(vec![handed.clone()], vec![within.clone()]);
    assert_eq!(judged(&edge, 0).0, Carried::Out(Some(key(&within))));
    let beyond = pull(vec![handed], vec![mark(12, "h", -skew - 1)]);
    assert_eq!(judged(&beyond, 0), (Carried::Out(None), None));
    let held = pull(vec![mark(10, "h", 0)], vec![mark(12, "h", -skew - 1)]);
    assert_eq!(judged(&held, 0), (Carried::Stands, None));
    let cancel = mark(12, "h", 120);
    let close = pull(
        vec![mark(20, "a", 120 + skew), mark(10, "h", 0)],
        vec![cancel.clone()],
    );
    assert_eq!(
        judged(&close, 1).0,
        Carried::Out(Some(key(&cancel))),
        "a gesture within the skew after the cancel does not stand"
    );
    let past = pull(
        vec![mark(20, "a", 121 + skew), mark(10, "h", 0)],
        vec![cancel],
    );
    assert_eq!(judged(&past, 1), (Carried::Stands, None));
}

#[test]
fn a_gesture_is_herald_s_by_its_id_and_time_and_its_maker_is_read_by_the_forge_s_name_now() {
    let mut taken = taken_back(None);
    let record = kept(&taken);
    assert_eq!(newer(&taken, &record), Some(0));
    assert_eq!(
        holds(&taken, &taken.gestures[0]),
        Some(Holds::Role(Permission::Write))
    );
    assert_eq!(renamed(&taken, &record), None);
    let other = GestureRecord {
        key: GestureKey {
            gesture: 11,
            ..record.key.clone()
        },
        ..record.clone()
    };
    assert_eq!(newer(&taken, &other), None, "id");
    assert_eq!(renamed(&taken, &other), None);
    let later = GestureRecord {
        at: record.at + SignedDuration::from_secs(1),
        ..record.clone()
    };
    assert_eq!(newer(&taken, &later), None, "time");
    taken.gestures.insert(0, mark(11, "w", 0));
    assert_eq!(newer(&taken, &record), Some(1));
    taken.gestures[1].by = "ghost".to_owned();
    taken.makers.insert("ghost".to_owned(), Holds::Nothing);
    assert_eq!(holds(&taken, &taken.gestures[1]), Some(Holds::Nothing));
    assert_eq!(renamed(&taken, &record), Some("w".to_owned()));
    taken.makers.clear();
    assert_eq!(
        holds(&taken, &taken.gestures[1]),
        None,
        "a role the read holds none of"
    );
    taken.gestures[1].by = String::new();
    assert_eq!(
        holds(&taken, &taken.gestures[1]),
        Some(Holds::Nothing),
        "no one named"
    );
}

#[test]
fn each_outcome_of_a_take_out_says_what_this_apply_did() {
    assert_eq!(took(TakenOut::Out, None, None), Undo::Took(TookOut::Out));
    assert_eq!(
        took(TakenOut::Refused, Some("no".to_owned()), None),
        Undo::Took(TookOut::Declined("no".to_owned()))
    );
    assert_eq!(
        took(TakenOut::Refused, None, None),
        Undo::Took(TookOut::Declined(String::new()))
    );
    assert_eq!(
        took(TakenOut::Unsettled, None, None),
        Undo::Took(TookOut::Unsettled)
    );
    for outcome in [TakenOut::Calling, TakenOut::Unsent] {
        assert_eq!(
            took(outcome, None, Some("why".to_owned())),
            Undo::Uncarried("why".to_owned())
        );
        assert_eq!(
            took(outcome, None, None),
            Undo::Uncarried(outcome.said().to_owned())
        );
    }
}

#[test]
fn a_pull_request_read_in_part_is_shown_and_one_herald_handed_nothing_for_is_not() {
    let mut partial = taken_back(Some(Permission::Write));
    partial.whole = false;
    let (cancel, kept) = pollster::block_on(undone(
        None::<&herald_testing::FakeStore>,
        &queue(Runs::Forge),
        partial.clone(),
    ))
    .unwrap();
    assert_eq!(cancel.next, Undo::Uncarried(UNREAD_CANCEL.to_owned()));
    assert_eq!(cancel.cancel, None);
    assert_eq!(kept, None);
    let mut unheld = partial;
    unheld.whole = true;
    assert_eq!(
        pollster::block_on(undone(
            None::<&herald_testing::FakeStore>,
            &queue(Runs::Forge),
            unheld,
        )),
        None,
        "nothing herald handed over: nothing to say"
    );
}
