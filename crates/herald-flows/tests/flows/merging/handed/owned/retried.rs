//! herald's own queue re-runs the failed job of a check its `retries` name,
//! at most `times` on one head, before the pull request leaves.

use herald_core::merging::{Left, Step, StepKind, Stepped, Verdict};
use herald_flows::{Acted, Budget, Next};
use herald_ports::Response;
use serde_json::{Value, json};

use super::{
    Forge, OWN, PULL, answer, applied, checks, github, kept_step, left, own_pull, record,
    run_config, the_secrets,
};
use crate::harness::{Failing, healthy};

const RETRIES: &str = "retries = { checks = [\"ci\"], times = 1 }\n";
const RUNS: &str =
    "GET https://api.github.com/repos/o/r/actions/runs?head_sha=h7&status=failure&per_page=5";
const JOBS: &str =
    "GET https://api.github.com/repos/o/r/actions/runs/42/jobs?filter=latest&per_page=100";
const RERUN: &str = "POST https://api.github.com/repos/o/r/actions/jobs/420/rerun";

/// The check runs on `h7`, each by its name and conclusion.
fn ran(conclusions: &[(&str, &str)]) -> Response {
    let runs: Vec<Value> = conclusions
        .iter()
        .map(|(name, conclusion)| json!({ "name": name, "status": "completed", "conclusion": conclusion }))
        .collect();
    answer(
        200,
        &json!({ "total_count": runs.len(), "check_runs": runs }),
    )
}

/// The failed runs on `h7`: run 42 where `failed`, else none.
fn failed_runs(failed: bool) -> Response {
    let runs = if failed {
        json!([{ "id": 42, "html_url": "https://github.com/o/r/actions/runs/42" }])
    } else {
        json!([])
    };
    answer(200, &json!({ "workflow_runs": runs }))
}

/// GitHub with pull request 7 up to date on `h7`, its check `ci` failed,
/// run 42's job 420 of it failed, and the re-run answered `rerun`.
fn failing(rerun: &[Response]) -> Forge {
    github("h7", &[], 0)
        .answering(&checks("h7").0, &[ran(&[("ci", "failure")])])
        .answering(RUNS, &[failed_runs(true)])
        .answering(
            JOBS,
            &[answer(
                200,
                &json!({ "jobs": [{ "id": 420, "name": "ci", "conclusion": "failure" }] }),
            )],
        )
        .answering(RERUN, rerun)
}

/// Each step recorded, as its kind, the check it re-ran and its outcome.
fn reruns(store: &Failing) -> Vec<(StepKind, Option<String>, Stepped)> {
    record(store)
        .map(|record| record.own.steps)
        .unwrap_or_default()
        .into_iter()
        .map(|step| (step.kind, step.check, step.outcome))
        .collect()
}

#[test]
fn a_failed_check_the_retries_name_is_re_run_by_its_job_alone_at_most_times_on_one_head() {
    let store = healthy();
    let forge = failing(&[answer(201, &json!({}))]);
    assert_eq!(
        applied(RETRIES, (&store, &forge), false),
        [Next::Verdict(Verdict::Rerun)]
    );
    assert!(!forge.lines().iter().any(|line| line.starts_with("POST")));
    assert_eq!(
        applied(RETRIES, (&store, &forge), true),
        [Next::Acted(Acted::Stepped(StepKind::Rerun, Stepped::Took))]
    );
    assert_eq!(forge.carrying("write-value"), [RERUN.to_owned()]);
    let once = (StepKind::Rerun, Some("ci".to_owned()), Stepped::Took);
    assert_eq!(reruns(&store), std::slice::from_ref(&once));
    assert_eq!(
        applied(RETRIES, (&store, &forge), true),
        [Next::Acted(Acted::Left(Left::ChecksFailed))]
    );
    assert_eq!(forge.carrying("write-value"), [RERUN.to_owned()]);
    assert_eq!(
        (reruns(&store), left(&store)),
        (vec![once], Some(Left::ChecksFailed))
    );
    let twice = RETRIES.replace("times = 1", "times = 2");
    let store = healthy();
    let forge = failing(&[answer(201, &json!({}))]);
    applied(&twice, (&store, &forge), true);
    applied(&twice, (&store, &forge), true);
    assert_eq!(forge.carrying("write-value"), [RERUN, RERUN]);
}

#[test]
fn a_re_run_never_sent_is_sent_again_and_one_on_another_head_or_check_counts_not() {
    let step = |head: &str, check: &str, outcome| Step {
        kind: StepKind::Rerun,
        head: head.to_owned(),
        base: "b1".to_owned(),
        clean: true,
        made: None,
        check: Some(check.to_owned()),
        credential: "HERALD_MERGE".to_owned(),
        at: "2026-11-01T11:59:00Z".parse().unwrap(),
        outcome,
    };
    for kept in [
        step("h7", "ci", Stepped::Unsent),
        step("h6", "ci", Stepped::Took),
        step("h7", "lint", Stepped::Took),
    ] {
        let store = healthy();
        kept_step(&store, kept.clone());
        let forge = failing(&[answer(201, &json!({}))]);
        assert_eq!(
            applied(RETRIES, (&store, &forge), true),
            [Next::Acted(Acted::Stepped(StepKind::Rerun, Stepped::Took))],
            "{kept:?}"
        );
    }
}

#[test]
fn a_failed_check_the_retries_do_not_name_or_no_failed_run_ran_leaves_at_once() {
    let cases = [
        (
            "retries = { checks = [\"other\"], times = 1 }\n",
            failing(&[answer(201, &json!({}))]),
        ),
        (
            RETRIES,
            failing(&[answer(201, &json!({}))]).answering(
                &checks("h7").0,
                &[ran(&[("ci", "failure"), ("lint", "failure")])],
            ),
        ),
        (
            RETRIES,
            failing(&[answer(201, &json!({}))]).answering(RUNS, &[failed_runs(false)]),
        ),
        ("", failing(&[answer(201, &json!({}))])),
    ];
    for (retries, forge) in cases {
        let store = healthy();
        assert_eq!(
            applied(retries, (&store, &forge), true),
            [Next::Acted(Acted::Left(Left::ChecksFailed))],
            "{retries}"
        );
        assert!(forge.carrying("write-value").is_empty(), "{retries}");
        assert_eq!(reruns(&store), [], "{retries}");
    }
}

#[test]
fn a_refused_re_run_leaves_and_a_lost_answer_is_read_back_by_the_failed_runs() {
    let store = healthy();
    let forge = failing(&[answer(
        403,
        &json!({ "message": "This job cannot be rerun" }),
    )]);
    assert_eq!(
        applied(RETRIES, (&store, &forge), true),
        [Next::Acted(Acted::Stepped(
            StepKind::Rerun,
            Stepped::Refused
        ))]
    );
    assert_eq!(left(&store), Some(Left::ChecksFailed));
    for (back, outcome) in [
        (failed_runs(true), Stepped::Unsettled),
        (failed_runs(false), Stepped::Took),
        (answer(500, &json!({})), Stepped::Unsettled),
    ] {
        let store = healthy();
        let forge = failing(&[]).answering(RUNS, &[failed_runs(true), back]);
        assert_eq!(
            applied(RETRIES, (&store, &forge), true),
            [Next::Acted(Acted::Stepped(StepKind::Rerun, outcome))]
        );
    }
}

#[test]
fn a_write_app_mints_its_re_run_token_with_actions_write_alone() {
    let tokens = "POST https://api.github.com/app/installations/3/access_tokens";
    let minted = json!({ "token": "minted-value", "expires_at": "2099-01-01T00:00:00Z",
        "permissions": { "actions": "write", "metadata": "read" }, "repositories": [{ "name": "r" }] });
    let forge = failing(&[answer(201, &json!({}))])
        .answering(
            "GET https://api.github.com/repos/o/r/installation",
            &[answer(200, &json!({ "id": 3, "app_id": 40 }))],
        )
        .answering(tokens, &[answer(201, &minted)]);
    let app = "[github_apps.merger]\nclient_id = \"Iv23liMerger\"\nprivate_key = { secret = \"HERALD_MERGER_KEY\" }\n";
    let config = format!("{OWN}{RETRIES}{app}").replace(
        "write = { token = { secret = \"HERALD_MERGE\" } }",
        "write = { app = \"merger\" }",
    );
    let store = healthy();
    let held = the_secrets().with("HERALD_MERGER_KEY", "the App's key");
    assert_eq!(
        run_config(&config, (&store, &forge, &held), (true, Budget::UNBOUNDED)),
        [Next::Acted(Acted::Stepped(StepKind::Rerun, Stepped::Took))]
    );
    assert_eq!(
        forge.bodies(tokens)[0]["permissions"],
        json!({ "actions": "write" })
    );
    assert_eq!(forge.carrying("minted-value"), [RERUN.to_owned()]);
}

/// Run `run`'s jobs, its job `run`0 of `ci` failed.
fn jobs_of(run: u64) -> (String, Response) {
    (
        format!(
            "GET https://api.github.com/repos/o/r/actions/runs/{run}/jobs?filter=latest&per_page=100"
        ),
        answer(
            200,
            &json!({ "jobs": [{ "id": run * 10, "name": "ci", "conclusion": "failure" }] }),
        ),
    )
}

/// The failed runs on `h7`, newest first.
fn runs_of(runs: &[u64]) -> Response {
    let runs: Vec<Value> = runs
        .iter()
        .map(|run| json!({ "id": run, "html_url": format!("https://github.com/o/r/actions/runs/{run}") }))
        .collect();
    answer(200, &json!({ "workflow_runs": runs }))
}

#[test]
fn the_newest_failed_run_is_re_run_and_another_still_failed_reads_back_as_re_run() {
    let (older, older_jobs) = jobs_of(41);
    let (other, other_jobs) = jobs_of(43);
    let store = healthy();
    let forge = failing(&[answer(201, &json!({}))])
        .answering(RUNS, &[runs_of(&[42, 41])])
        .answering(&older, &[older_jobs]);
    applied(RETRIES, (&store, &forge), true);
    assert_eq!(forge.carrying("write-value"), [RERUN.to_owned()]);
    let store = healthy();
    let forge = failing(&[])
        .answering(RUNS, &[runs_of(&[42]), runs_of(&[43])])
        .answering(&other, &[other_jobs]);
    assert_eq!(
        applied(RETRIES, (&store, &forge), true),
        [Next::Acted(Acted::Stepped(StepKind::Rerun, Stepped::Took))]
    );
}

#[test]
fn a_conflict_leaves_before_a_failed_check_is_re_run_and_no_write_credential_refuses() {
    let store = healthy();
    let dirty = answer(200, &own_pull("h7", (Some(false), "dirty"), false));
    let forge = failing(&[answer(201, &json!({}))])
        .answering(PULL, &[dirty])
        .answering(
            "GET https://api.github.com/repos/o/r/compare/b1...h7?per_page=1",
            &[answer(200, &json!({ "behind_by": 1 }))],
        );
    assert_eq!(
        applied(RETRIES, (&store, &forge), true),
        [Next::Acted(Acted::Left(Left::Conflict))]
    );
    let unwritten = the_secrets().with("HERALD_MERGE", "read-value");
    let forge = failing(&[answer(201, &json!({}))]);
    let said = run_config(
        &format!("{OWN}{RETRIES}"),
        (&healthy(), &forge, &unwritten),
        (true, Budget::UNBOUNDED),
    );
    assert!(
        matches!(&said[..], [Next::Acted(Acted::Refused(_))]),
        "{said:?}"
    );
    assert!(!forge.lines().iter().any(|line| line.starts_with("POST")));
}
