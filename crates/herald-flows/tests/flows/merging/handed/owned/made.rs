//! herald's own update records the head read right after it where the
//! forge made and signed that head from the head and base the update was
//! taken on, and a gesture vouches for that recorded head alone.

use herald_core::merging::{Left, Step, StepKind, Stepped, UPDATE_PAUSE};
use herald_flows::{Acted, Budget, Next, Requests};
use herald_ports::Response;
use serde_json::json;

use super::{
    ACTIVITY, EVENTS, Failing, Fault, Forge, MERGE, OWN, PERMISSION, PULL, UPDATE, after, answer,
    applied, carried, commit, github, healthy, open, paused, reads, record, run_config, steps,
    taken, the_secrets,
};

const SIGNED: (bool, &str) = (true, "noreply@github.com");

/// What the read right after the update answers, how GitHub made `h8`,
/// its parents, and what the case is named.
type Unmade<'a> = (Response, (bool, &'a str), &'a [&'a str], &'a str);

/// The run bringing `h7` up to date with `b1` under the queue's `extra`
/// settings, where the read of the pull request after the first `before`
/// answers `then` and GitHub says it made `h8` as `made` with `parents`:
/// each next step, the head recorded as made, and the forge it ran against.
fn updated(
    (extra, before): (&str, usize),
    then: Response,
    (made, parents): ((bool, &str), &[&str]),
) -> (Vec<Next>, Option<String>, Forge) {
    let (line, commit) = commit("h8", made, parents);
    let forge = after(github("h7", &[], 2), PULL, (open("h7"), before), then)
        .answering(&line, &[answer(200, &commit)]);
    let store = healthy();
    let nexts = applied(extra, (&store, &forge), true);
    let made = record(&store).and_then(|record| record.own.steps[0].made.clone());
    (nexts, made, forge)
}

/// Each request and pause after the update, in turn, as `forge` was sent
/// them.
fn after_update(forge: &Forge) -> Vec<String> {
    let lines = forge.lines();
    let at = lines.iter().position(|line| line == UPDATE).unwrap();
    lines[at + 1..].to_vec()
}

/// The head `store` records the update made.
fn made_in(store: &Failing) -> Option<String> {
    record(store).and_then(|record| record.own.steps[0].made.clone())
}

/// The requests one read of the pull request, its gesture and who made
/// it, sends.
fn read_once() -> Vec<String> {
    [PULL, EVENTS, PERMISSION, ACTIVITY]
        .map(ToOwned::to_owned)
        .to_vec()
}

/// A read of the pull request after each of `pauses` pauses, the first
/// one before them.
fn read_after(pauses: usize) -> Vec<String> {
    let pause = paused(UPDATE_PAUSE.unsigned_abs());
    let mut lines = read_once();
    for _ in 0..pauses {
        lines.push(pause.clone());
        lines.extend(read_once());
    }
    lines
}

/// How often a run that brings `h7` up to date reads the pull request
/// before the read right after its update.
fn pulls() -> usize {
    let probe = github("h7", &[], 2);
    applied("", (&healthy(), &probe), true);
    let lines = probe.lines();
    let updated = lines.iter().take_while(|line| *line != UPDATE);
    updated.filter(|line| *line == PULL).count() + 1
}

#[test]
fn a_branch_behind_is_brought_up_to_date_with_the_head_it_must_find_then_merged_on_the_forge_s_signed_merge()
 {
    let store = healthy();
    let probe = github("h7", &[], 2);
    applied("", (&healthy(), &probe), true);
    let (line, made) = commit("h8", SIGNED, &["h7", "b1"]);
    let before = reads(&probe, PULL) - 1;
    let forge = after(github("h7", &[], 2), PULL, (open("h7"), before), open("h8"))
        .answering(&line, &[answer(200, &made)]);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Stepped(StepKind::Update, Stepped::Took))]
    );
    assert_eq!(forge.carrying("write-value"), [UPDATE]);
    assert_eq!(forge.bodies(UPDATE)[0]["expected_head_sha"], "h7");
    assert_eq!(
        steps(&store),
        [(
            StepKind::Update,
            "h7".to_owned(),
            "b1".to_owned(),
            true,
            Stepped::Took
        )]
    );
    let forge = github("h8", &["h7"], 0).answering(&line, &[answer(200, &made)]);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))]
    );
    assert_eq!(forge.bodies(MERGE)[0]["sha"], "h8");
}

#[test]
fn an_update_records_the_head_read_right_after_it_only_where_the_forge_made_it_from_the_step_s_heads()
 {
    let took = [Next::Acted(Acted::Stepped(StepKind::Update, Stepped::Took))];
    let right_after = ("", pulls() - 1);
    let (nexts, made, _) = updated(right_after, open("h8"), (SIGNED, &["h7", "b1"]));
    assert_eq!((nexts, made), (took.to_vec(), Some("h8".to_owned())));
    let unmade: [Unmade; 6] = [
        (
            open("h8"),
            (false, "noreply@github.com"),
            &["h7", "b1"],
            "unsigned",
        ),
        (
            open("h8"),
            (true, "someone@example.com"),
            &["h7", "b1"],
            "another signer",
        ),
        (
            open("h8"),
            SIGNED,
            &["b1", "h7"],
            "parents in the other order",
        ),
        (open("h8"), SIGNED, &["h7", "b9"], "another base"),
        (open("h7"), SIGNED, &["h7", "b1"], "the head not moved yet"),
        (
            answer(500, &json!({})),
            SIGNED,
            &["h7", "b1"],
            "the head unread",
        ),
    ];
    for (then, made, parents, case) in unmade {
        let (nexts, made, _) = updated(right_after, then, (made, parents));
        assert_eq!((nexts, made), (took.to_vec(), None), "{case}");
    }
    let never = "updated = \"never\"\n";
    let (nexts, made, forge) = updated((never, right_after.1), open("h8"), (SIGNED, &["h7", "b1"]));
    assert_eq!((nexts, made), (took.to_vec(), None));
    let (line, _) = commit("h8", SIGNED, &[]);
    assert_eq!(
        (reads(&forge, PULL), reads(&forge, &line)),
        (right_after.1, 0)
    );
}

#[test]
fn an_update_whose_outcome_cannot_be_recorded_is_never_taken_again() {
    let store = Failing {
        fault: Fault::SettlesNoAction,
        ..healthy()
    };
    let forge = github("h7", &[], 2);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Unrecorded(
            StepKind::Update,
            Stepped::Took
        ))]
    );
    let kept = record(&store).map(|record| record.own.steps);
    assert!(
        matches!(&kept.as_deref(), Some([step]) if step.outcome == Stepped::Calling && step.made.is_none()),
        "{kept:?}"
    );
    let again = applied("", (&store, &forge), true);
    assert!(matches!(&again[..], [Next::Held(_)]), "{again:?}");
    assert_eq!(forge.carrying("write-value"), [UPDATE]);
}

#[test]
fn a_gesture_vouches_only_for_the_head_recorded_right_after_herald_s_update() {
    let recorded = |made: Option<&str>| {
        Some(Step {
            made: made.map(ToOwned::to_owned),
            ..taken(StepKind::Update, ("h7", "b1"), true, Stepped::Took)
        })
    };
    let (nexts, _, _) = carried(recorded(Some("h8")), (SIGNED, &["h7", "b1"]), "");
    assert_eq!(
        nexts,
        [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))]
    );
    for made in [None, Some("h7"), Some("h9")] {
        let (nexts, gone, writes) = carried(recorded(made), (SIGNED, &["h7", "b1"]), "");
        assert_eq!(
            nexts,
            [Next::Acted(Acted::Left(Left::HeadMoved))],
            "{made:?}"
        );
        assert_eq!(
            (gone, writes),
            (Some(Left::HeadMoved), Vec::new()),
            "{made:?}"
        );
    }
}

#[test]
fn a_head_the_update_moves_within_update_wait_is_read_again_until_it_moved_then_recorded_and_carried()
 {
    let took = [Next::Acted(Acted::Stepped(StepKind::Update, Stepped::Took))];
    let (line, made) = commit("h8", SIGNED, &["h7", "b1"]);
    let store = healthy();
    let forge = after(
        github("h7", &[], 2),
        PULL,
        (open("h7"), pulls() + 1),
        open("h8"),
    )
    .answering(&line, &[answer(200, &made)]);
    assert_eq!(applied("", (&store, &forge), true), took);
    let mut expected = read_after(2);
    expected.push(line.clone());
    assert_eq!(after_update(&forge), expected);
    assert_eq!(made_in(&store), Some("h8".to_owned()));
    let merging = github("h8", &["h7"], 0).answering(&line, &[answer(200, &made)]);
    assert_eq!(
        applied("", (&store, &merging), true),
        [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))]
    );
    assert_eq!(merging.bodies(MERGE)[0]["sha"], "h8");
}

#[test]
fn a_head_still_unmoved_once_update_wait_is_paused_through_records_none() {
    let took = [Next::Acted(Acted::Stepped(StepKind::Update, Stepped::Took))];
    for (extra, pauses) in [
        ("", 5),
        ("update_wait = \"5s\"\n", 2),
        ("update_wait = \"0s\"\n", 0),
        ("update_wait = \"1s\"\n", 0),
        ("update_wait = \"1m\"\n", 30),
    ] {
        let store = healthy();
        let forge = github("h7", &[], 2);
        assert_eq!(applied(extra, (&store, &forge), true), took, "{extra}");
        assert_eq!(after_update(&forge), read_after(pauses), "{extra}");
        assert_eq!(made_in(&store), None, "{extra}");
    }
}

#[test]
fn the_first_head_read_moved_is_proved_once_and_read_no_more_where_the_forge_did_not_make_it() {
    let took = [Next::Acted(Acted::Stepped(StepKind::Update, Stepped::Took))];
    let (line, unmade) = commit("h8", SIGNED, &["h7", "b9"]);
    let store = healthy();
    let forge = after(
        github("h7", &[], 2),
        PULL,
        (open("h7"), pulls()),
        open("h8"),
    )
    .answering(&line, &[answer(200, &unmade)]);
    assert_eq!(applied("", (&store, &forge), true), took);
    let mut expected = read_after(1);
    expected.push(line);
    assert_eq!(after_update(&forge), expected);
    assert_eq!(made_in(&store), None);
}

#[test]
fn no_head_is_read_after_the_update_once_the_run_s_budget_is_out() {
    let took = [Next::Acted(Acted::Stepped(StepKind::Update, Stepped::Took))];
    let probe = github("h7", &[], 2);
    applied("", (&healthy(), &probe), true);
    let updated_by = probe
        .lines()
        .iter()
        .position(|line| line == UPDATE)
        .unwrap()
        + 1;
    let mut once = read_once();
    once.push(paused(UPDATE_PAUSE.unsigned_abs()));
    for (reads, expected) in [(0, Vec::new()), (read_once().len(), once)] {
        let store = healthy();
        let forge = github("h7", &[], 2);
        let requests = Requests {
            made: &forge.made,
            from: 0,
            most: updated_by + reads,
            spare: 0,
        };
        let budget = Budget {
            requests: Some(requests),
            ..Budget::UNBOUNDED
        };
        let nexts = run_config(OWN, (&store, &forge, &the_secrets()), (true, budget));
        assert_eq!(nexts, took, "{reads}");
        assert_eq!(after_update(&forge), expected, "{reads}");
        assert_eq!(made_in(&store), None, "{reads}");
    }
}
