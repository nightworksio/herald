use std::collections::BTreeMap;

use herald_core::Permission;
use herald_core::merging::{CheckState, Runs};
use herald_kinds::{Marked, TakenBack};
use herald_ports::Version;
use herald_testing::ScriptedHttp;
use jiff::SignedDuration;

use super::super::tests::{Fixed, base, entry, green, now, queue, record};
use super::*;

fn behind() -> Proved {
    Proved {
        up_to_date: false,
        ..green(CheckState::Passed, true)
    }
}

fn step(kind: StepKind, head: &str, (outcome, minutes): (Stepped, i64)) -> Step {
    Step {
        kind,
        head: head.to_owned(),
        base: "b0".to_owned(),
        clean: true,
        made: None,
        check: None,
        credential: "HERALD_MERGE".to_owned(),
        at: now() - SignedDuration::from_mins(minutes),
        outcome,
    }
}

fn kept(steps: Vec<Step>, left: Option<Left>) -> Versioned<GestureRecord> {
    let mut value = record("h1", None);
    value.own = Own {
        steps,
        left: left.map(|left| Leaving { left, at: now() }),
    };
    Versioned {
        value,
        version: Version(1),
    }
}

fn next_of(
    reader: &Fixed,
    queue: &MergeQueue,
    (entered, kept): (&Entered, Option<&Versioned<GestureRecord>>),
) -> (Next, bool) {
    let http = ScriptedHttp::new(Vec::new());
    let mut flying = false;
    let next = pollster::block_on(next(
        (reader, &http),
        queue,
        (entered, None, kept),
        &base(),
        (now(), &mut flying),
    ))
    .unwrap();
    (next, flying)
}

fn methods(allowed: &[Method], whole: bool) -> Methods {
    Methods {
        allowed: allowed.to_vec(),
        whole,
    }
}

#[test]
fn a_method_is_taken_only_where_the_repository_allows_it_and_repository_takes_its_one() {
    let both = methods(&[Method::Squash, Method::Merge], true);
    assert_eq!(allowed(Some(Method::Squash), &both), Ok(Method::Squash));
    let refused = allowed(Some(Method::Rebase), &both).unwrap_err();
    assert!(
        refused.contains("\"rebase\"") && refused.contains("squash, merge"),
        "{refused}"
    );
    let one = allowed(Some(Method::Repository), &both).unwrap_err();
    assert!(one.contains("squash, merge"), "{one}");
    let single = methods(&[Method::Rebase], true);
    assert_eq!(
        allowed(Some(Method::Repository), &single),
        Ok(Method::Rebase)
    );
    assert_eq!(
        allowed(Some(Method::Squash), &methods(&[Method::Squash], false)),
        Err(METHODS_UNREAD.to_owned())
    );
    assert_eq!(
        allowed(None, &both),
        Err(super::super::METHODLESS.to_owned())
    );
}

#[test]
fn a_recorded_leaving_or_a_step_the_forge_may_have_taken_holds_the_entry() {
    let reader = Fixed::proving(vec![(1, green(CheckState::Passed, true))]);
    let own = queue(Runs::Herald);
    let entered = entry(1, ("h1", "h1"), true);
    let left = kept(vec![], Some(Left::ChecksFailed));
    assert_eq!(
        next_of(&reader, &own, (&entered, Some(&left))),
        (Next::Verdict(Verdict::Leaves(Left::ChecksFailed)), false)
    );
    for outcome in [Stepped::Calling, Stepped::Unsettled] {
        let unknown = step(StepKind::Merge, "h1", (outcome, 1));
        let held = kept(vec![unknown.clone()], None);
        assert_eq!(
            next_of(&reader, &own, (&entered, Some(&held))),
            (Next::Held(unknown), true)
        );
    }
    let sent = kept(vec![step(StepKind::Merge, "h0", (Stepped::Took, 1))], None);
    assert_eq!(
        next_of(&reader, &own, (&entered, Some(&sent))),
        (Next::Verdict(Verdict::Merge), true)
    );
}

#[test]
fn an_update_sent_on_a_head_waits_for_the_forge_until_the_queue_s_wait_has_passed() {
    let reader = Fixed::proving(vec![(1, behind())]);
    let own = queue(Runs::Herald);
    let entered = entry(1, ("h1", "h1"), true);
    assert_eq!(
        next_of(&reader, &own, (&entered, None)),
        (Next::Verdict(Verdict::Update), true)
    );
    let recent = step(StepKind::Update, "h1", (Stepped::Took, 1));
    let waiting = kept(vec![recent.clone()], None);
    assert_eq!(
        next_of(&reader, &own, (&entered, Some(&waiting))),
        (Next::Held(recent), true)
    );
    let minutes = own.wait.as_secs() / 60;
    let old = kept(
        vec![step(StepKind::Update, "h1", (Stepped::Took, minutes))],
        None,
    );
    assert_eq!(
        next_of(&reader, &own, (&entered, Some(&old))),
        (Next::Verdict(Verdict::Leaves(Left::CannotUpdate)), false)
    );
    let unsent = kept(
        vec![step(StepKind::Update, "h1", (Stepped::Unsent, minutes))],
        None,
    );
    assert_eq!(
        next_of(&reader, &own, (&entered, Some(&unsent))),
        (Next::Verdict(Verdict::Update), true)
    );
}

#[test]
fn a_gesture_vouches_for_herald_s_own_update_only_where_every_condition_holds() {
    let updated_from = |head: &str, base: &str| {
        let mut taken = step(StepKind::Update, head, (Stepped::Took, 5));
        base.clone_into(&mut taken.base);
        taken.made = Some("h2".to_owned());
        kept(vec![taken], None)
    };
    let made = |parents: Option<[&str; 2]>| Fixed {
        made: parents.map(|parents| parents.map(ToOwned::to_owned).to_vec()),
        ..Fixed::proving(vec![(1, green(CheckState::Passed, true))])
    };
    let entered = entry(1, ("h1", "h2"), true);
    let signed = queue(Runs::Herald);
    let moved = (Next::Verdict(Verdict::Leaves(Left::HeadMoved)), false);
    let carried = (Next::Verdict(Verdict::Merge), true);
    let record = updated_from("h1", "b0");
    let reader = made(Some(["h1", "b0"]));
    assert_eq!(
        next_of(&reader, &signed, (&entered, Some(&record))),
        carried
    );
    assert_eq!(next_of(&reader, &signed, (&entered, None)), moved);
    let mut conflicted = record.clone();
    conflicted.value.own.steps[0].clean = false;
    assert_eq!(
        next_of(&reader, &signed, (&entered, Some(&conflicted))),
        moved
    );
    for made in [None, Some("h1".to_owned()), Some("h3".to_owned())] {
        let mut unbound = record.clone();
        unbound.value.own.steps[0].made = made;
        assert_eq!(next_of(&reader, &signed, (&entered, Some(&unbound))), moved);
    }
    let mut merged = record.clone();
    merged.value.own.steps[0].kind = StepKind::Merge;
    assert_eq!(next_of(&reader, &signed, (&entered, Some(&merged))), moved);
    for outcome in [
        Stepped::Refused,
        Stepped::Unsent,
        Stepped::Calling,
        Stepped::Unsettled,
        Stepped::Conflict,
    ] {
        let mut unmade = record.clone();
        unmade.value.own.steps[0].outcome = outcome;
        let (next, _) = next_of(&reader, &signed, (&entered, Some(&unmade)));
        assert!(
            !matches!(next, Next::Verdict(Verdict::Merge)),
            "{outcome:?}"
        );
    }
    for parents in [
        None,
        Some(["b0", "h1"]),
        Some(["h1", "b9"]),
        Some(["h9", "b0"]),
        Some(["h1b0", "b0"]),
    ] {
        let reader = made(parents);
        assert_eq!(
            next_of(&reader, &signed, (&entered, Some(&record))),
            moved,
            "{parents:?}"
        );
    }
    let never = MergeQueue {
        updated: Updated::Never,
        ..signed.clone()
    };
    assert_eq!(next_of(&reader, &never, (&entered, Some(&record))), moved);
    let approving = MergeQueue {
        updated: Updated::Approved,
        ..signed
    };
    assert_eq!(
        next_of(&reader, &approving, (&entered, Some(&record))),
        moved
    );
    let approved = Entered {
        approved: true,
        ..entered.clone()
    };
    assert_eq!(
        next_of(&made(None), &approving, (&approved, Some(&record))),
        carried
    );
    assert_eq!(next_of(&made(None), &approving, (&approved, None)), moved);
}

/// A mark with id `id` by `by`, `mins` minutes before now.
fn marked(id: u64, by: &str, mins: i64) -> Marked {
    Marked {
        id: Some(id),
        by: by.to_owned(),
        at: now() - SignedDuration::from_mins(mins),
    }
}

/// The pull request read whole with these marks, newest first: `w`, who
/// made the gesture judged due, holds `holds`, `h` maintain, `r` nothing.
fn marks(gestures: Vec<Marked>, cancels: Vec<Marked>, holds: Holds) -> TakenBack {
    TakenBack {
        number: 1,
        url: "u1".to_owned(),
        id: None,
        gestures,
        cancels,
        makers: BTreeMap::from([
            ("w".to_owned(), holds),
            ("h".to_owned(), Holds::Role(Permission::Maintain)),
            ("r".to_owned(), Holds::Nothing),
        ]),
        whole: true,
    }
}

/// What the gesture of [`entry`] comes to in a queue whose least role is
/// `least`, its marks read as `took`, none read where it is none.
fn unstood_of(took: Option<Vec<TakenBack>>, least: Permission) -> Result<Left, String> {
    let reader = Fixed {
        took,
        ..Fixed::default()
    };
    let http = ScriptedHttp::new(Vec::new());
    let queue = MergeQueue {
        least,
        ..queue(Runs::Herald)
    };
    let entered = entry(1, ("h1", "h1"), true);
    pollster::block_on(unstood((&reader, &http), &queue, &entered))
}

#[test]
fn a_role_lost_since_the_queue_s_read_leaves_recorded_and_one_unsaid_or_unread_does_not() {
    let anchor = || vec![marked(70, "w", 30)];
    let gone = Err(GONE.to_owned());
    for (holds, least, left) in [
        (Holds::Nothing, Permission::Write, Ok(Left::Removed)),
        (
            Holds::Role(Permission::Write),
            Permission::Maintain,
            Ok(Left::Removed),
        ),
        (
            Holds::Role(Permission::Write),
            Permission::Write,
            gone.clone(),
        ),
        (Holds::Unsaid, Permission::Write, Err(OWN_UNSAID.to_owned())),
    ] {
        let took = marks(anchor(), Vec::new(), holds);
        assert_eq!(unstood_of(Some(vec![took]), least), left, "{holds:?}");
    }
    let ghost = marks(vec![marked(70, "", 30)], Vec::new(), Holds::Unsaid);
    assert_eq!(
        unstood_of(Some(vec![ghost]), Permission::Write),
        Ok(Left::Removed)
    );
    let unknown = marks(vec![marked(70, "z", 30)], Vec::new(), Holds::Nothing);
    assert_eq!(
        unstood_of(Some(vec![unknown]), Permission::Write),
        Err(OWN_UNSAID.to_owned())
    );
    let partial = TakenBack {
        whole: false,
        ..marks(anchor(), Vec::new(), Holds::Nothing)
    };
    assert_eq!(
        unstood_of(Some(vec![partial]), Permission::Write),
        Err(UNREAD_AGAIN.to_owned())
    );
    assert_eq!(unstood_of(Some(Vec::new()), Permission::Write), gone);
    let unread = unstood_of(None, Permission::Write).unwrap_err();
    assert!(unread.contains("could not be read again"), "{unread}");
    for other in [marked(71, "w", 30), marked(70, "w", 29)] {
        let took = marks(vec![other], Vec::new(), Holds::Nothing);
        assert_eq!(unstood_of(Some(vec![took]), Permission::Write), gone);
    }
}

#[test]
fn only_a_mark_after_the_gesture_by_one_holding_the_least_role_takes_no_step() {
    let gone = Err(GONE.to_owned());
    let removed = Ok(Left::Removed);
    let skewed = Marked {
        at: now() - SignedDuration::from_mins(30) - SignedDuration::from_secs(3),
        ..marked(9, "h", 0)
    };
    for (gestures, cancels, left) in [
        (vec![marked(71, "h", 5)], Vec::new(), gone.clone()),
        (vec![marked(71, "r", 5)], Vec::new(), removed.clone()),
        (Vec::new(), vec![marked(9, "h", 5)], gone.clone()),
        (Vec::new(), vec![skewed], gone),
        (Vec::new(), vec![marked(9, "h", 31)], removed.clone()),
        (Vec::new(), vec![marked(9, "r", 5)], removed),
    ] {
        let mut gestures = gestures;
        gestures.push(marked(70, "w", 30));
        let took = marks(gestures, cancels, Holds::Nothing);
        assert_eq!(
            unstood_of(Some(vec![took.clone()]), Permission::Write),
            left,
            "{took:?}"
        );
    }
}

#[test]
fn a_fresh_read_whose_gesture_maker_holds_no_role_now_judges_every_mark_on_it() {
    let mut fresh = entry(1, ("h1", "h1"), true);
    fresh.gesture.role = None;
    let reader = Fixed {
        fresh: Some(fresh),
        took: Some(vec![marks(
            vec![marked(70, "w", 30)],
            Vec::new(),
            Holds::Nothing,
        )]),
        ..Fixed::default()
    };
    let http = ScriptedHttp::new(Vec::new());
    let entered = entry(1, ("h1", "h1"), true);
    let read = pollster::block_on(fresh_read(
        (&reader, &http),
        &queue(Runs::Herald),
        (&entered, None, StepKind::Update),
        now(),
    ));
    assert!(matches!(read, Err(Ok(Left::Removed))));
}
