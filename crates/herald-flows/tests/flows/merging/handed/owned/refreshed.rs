//! herald's own queue takes no step where the gesture read again right
//! before it is not the one judged, or was not read whole.

use herald_core::merging::Left;
use herald_core::merging::Verdict;
use herald_core::merging::{StepKind, Stepped};
use herald_flows::{Acted, Next, OWN_REGESTURED, UNDUE, UNNAMED, UNREAD_AGAIN};
use herald_ports::Response;
use serde_json::{Value, json};

use super::{
    ACTIVITY, EVENTS, Forge, answer, applied, checks, commit, github, kept_step, label, labeled,
    left, reads, steps, taken,
};
use crate::harness::{Failing, Fault, healthy};

const BRANCH: &str = "GET https://api.github.com/repos/o/r/branches/main";
const RULES: &str = "GET https://api.github.com/repos/o/r/rules/branches/main?per_page=100&page=1";

/// GitHub as [`github`] has it, answering `line` as `normal` up to the read
/// right before the step, and that read as `then`.
fn at_the_step(line: &str, normal: &Response, then: Response) -> Forge {
    let probe = github("h7", &[], 0);
    applied("", (&healthy(), &probe), false);
    let mut answers = vec![normal.clone(); reads(&probe, line)];
    answers.push(then);
    github("h7", &[], 0).answering(line, &answers)
}

#[test]
fn a_push_record_or_a_base_read_again_that_moved_or_failed_takes_no_step() {
    let pushed = |pushes: Value| answer(200, &pushes);
    let before = pushed(
        json!([{ "before": "h6", "timestamp": "2026-11-01T10:00:00Z", "activity_type": "push" }]),
    );
    let moved = pushed(json!([
        { "before": "h6", "timestamp": "2026-11-01T11:30:00Z", "activity_type": "push" },
        { "before": "h5", "timestamp": "2026-11-01T10:00:00Z", "activity_type": "push" }
    ]));
    let store = healthy();
    let forge = at_the_step(ACTIVITY, &before, moved);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Left(Left::HeadMoved))]
    );
    assert_eq!(left(&store), Some(Left::HeadMoved));
    let branch = answer(
        200,
        &json!({ "commit": { "sha": "b1" }, "protected": false }),
    );
    let rules: Vec<Value> = (0..100).map(|_| json!({ "type": "deletion" })).collect();
    let green = answer(
        200,
        &json!({ "total_count": 1, "check_runs": [{ "name": "ci", "status": "completed", "conclusion": "success" }] }),
    );
    let cases = [
        at_the_step(&checks("h7").0, &green, answer(500, &json!({}))),
        at_the_step(ACTIVITY, &before, answer(500, &json!({}))),
        at_the_step(BRANCH, &branch, answer(500, &json!({}))),
        at_the_step(RULES, &answer(200, &json!([])), answer(200, &json!(rules))),
    ];
    for (forge, unread) in cases.iter().zip([false, false, false, true]) {
        let store = healthy();
        let said = applied("", (&store, forge), true);
        let refused = match &said[..] {
            [Next::Acted(Acted::Refused(why))] => why.clone(),
            _ => panic!("{said:?}"),
        };
        if unread {
            assert_eq!(refused, UNREAD_AGAIN);
        } else {
            assert!(
                refused.starts_with("it could not be read again right before the step"),
                "{refused}"
            );
        }
        assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
        assert_eq!(steps(&store), []);
    }
}

/// A page of 100 events on pull request 7, none of them a label.
fn unlabelled() -> Value {
    let events: Vec<Value> = (0..100)
        .map(|id| json!({ "id": 1000 + id, "event": "subscribed", "actor": { "login": "x", "id": 3 }, "created_at": "2026-11-01T09:00:00Z" }))
        .collect();
    json!(events)
}

#[test]
fn a_gesture_made_again_or_events_not_read_whole_right_before_the_step_take_none() {
    let probe = github("h7", &[], 0);
    applied("", (&healthy(), &probe), false);
    let before = reads(&probe, EVENTS);
    let mut full = unlabelled();
    full[0] = labeled();
    let regestured = json!([label("labeled", 502, "11:05")]);
    for (then, said) in [(regestured, OWN_REGESTURED), (full, UNREAD_AGAIN)] {
        let mut queued = vec![answer(200, &json!([labeled()])); before];
        queued.push(answer(200, &then));
        let mut forge = github("h7", &[], 0).answering(EVENTS, &queued);
        for page in 2..=5 {
            let line = format!(
                "GET https://api.github.com/repos/o/r/issues/7/events?per_page=100&page={page}"
            );
            forge = forge.answering(&line, &[answer(200, &unlabelled())]);
        }
        let store = healthy();
        assert_eq!(
            applied("", (&store, &forge), true),
            [Next::Acted(Acted::Refused(said.to_owned()))]
        );
        assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
        assert_eq!(steps(&store), []);
    }
}

#[test]
fn a_check_failed_by_the_read_right_before_the_merge_leaves_or_is_due_a_re_run_not_a_merge() {
    let ran = |conclusion: &str| {
        answer(
            200,
            &json!({ "total_count": 1, "check_runs": [{ "name": "ci", "status": "completed", "conclusion": conclusion }] }),
        )
    };
    let line = checks("h7").0;
    let retries = "retries = { checks = [\"ci\"], times = 1 }\n";
    for (extra, then) in [
        ("", Next::Acted(Acted::Left(Left::ChecksFailed))),
        (retries, Next::Acted(Acted::Refused(UNDUE.to_owned()))),
    ] {
        let store = healthy();
        let forge = at_the_step(&line, &ran("success"), ran("failure"));
        assert_eq!(applied(extra, (&store, &forge), true), [then], "{extra}");
        assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    }
}

#[test]
fn a_commit_of_herald_s_own_update_unread_right_before_the_merge_takes_no_step() {
    let update = || taken(StepKind::Update, ("h7", "b1"), true, Stepped::Took);
    let (line, made) = commit("h8", (true, "noreply@github.com"), &["h7", "b1"]);
    let probe = github("h8", &["h7"], 0).answering(&line, &[answer(200, &made)]);
    let probed = healthy();
    kept_step(&probed, update());
    applied("", (&probed, &probe), false);
    let mut answers = vec![answer(200, &made); reads(&probe, &line)];
    answers.push(answer(500, &json!({})));
    let forge = github("h8", &["h7"], 0).answering(&line, &answers);
    let store = healthy();
    kept_step(&store, update());
    let said = applied("", (&store, &forge), true);
    assert!(
        matches!(&said[..], [Next::Acted(Acted::Refused(why))] if why.starts_with("it could not be read again right before the step")),
        "{said:?}"
    );
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
}

#[test]
fn a_gesture_with_no_id_or_a_store_that_cannot_write_records_nothing_and_takes_no_step() {
    let mut nameless = labeled();
    nameless.as_object_mut().unwrap().remove("id");
    let failed = answer(
        200,
        &json!({ "total_count": 1, "check_runs": [{ "name": "ci", "status": "completed", "conclusion": "failure" }] }),
    );
    let unnamed = github("h7", &[], 0).answering(EVENTS, &[answer(200, &json!([nameless]))]);
    let store = healthy();
    assert_eq!(
        applied("", (&store, &unnamed), true),
        [Next::Acted(Acted::Refused(UNNAMED.to_owned()))]
    );
    let leaves = Next::Verdict(Verdict::Leaves(Left::ChecksFailed));
    let unnamed = unnamed.answering(&checks("h7").0, std::slice::from_ref(&failed));
    assert_eq!(
        applied("", (&store, &unnamed), true),
        std::slice::from_ref(&leaves)
    );
    assert_eq!(steps(&store), []);
    let unwritable = Failing {
        fault: Fault::WritesActions,
        ..healthy()
    };
    let forge = github("h7", &[], 0).answering(&checks("h7").0, &[failed]);
    assert_eq!(applied("", (&unwritable, &forge), true), [leaves]);
}
