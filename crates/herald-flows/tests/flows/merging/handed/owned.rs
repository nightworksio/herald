//! herald's own queue: `herald merge-queue --apply` brings the pull request
//! in flight up to date and merges it, each step read again right before
//! the call, recorded first and settled after, and never taken twice.

use std::sync::{Arc, Mutex};

use herald_core::merging::{
    GestureKey, GestureRecord, Left, Own, Step, StepKind, Stepped, Verdict,
};
use herald_flows::{
    Acted, Budget, GONE, METHODS_UNREAD, Next, OWN_DRAFTED, OWN_UNSAID, RACED, UNDUE,
};
use herald_ports::{Actions as _, Response};
use serde_json::{Value, json};

use super::{
    EVENTS, Forge, GITHUB_API, PERMISSION, PULL, answer, gestured_at, key, label, labeled, paused,
    record, run_config, run_on, the_secrets, writer,
};
use crate::harness::{Failing, Fault, NOON, healthy};

mod made;
mod minted;
mod refreshed;
mod retried;
mod told;

const OWN: &str = r#"[sources.app]
kind = "github"
repository = "o/r"
token = { secret = "HERALD_READ" }
[merge_queues.app]
source = "app"
branch = "main"
runs = "herald"
method = "squash"
write = { token = { secret = "HERALD_MERGE" } }
"#;

const MERGE: &str = "PUT https://api.github.com/repos/o/r/pulls/7/merge";
const UPDATE: &str = "PUT https://api.github.com/repos/o/r/pulls/7/update-branch";
const ACTIVITY: &str = "GET https://api.github.com/repos/fork/r/activity?ref=refs%2Fheads%2Ftopic&direction=desc&per_page=100";

fn own_pull(head: &str, (mergeable, state): (Option<bool>, &str), merged: bool) -> Value {
    json!({
        "number": 7, "html_url": "https://github.com/o/r/pull/7", "node_id": "PR_7",
        "state": if merged { "closed" } else { "open" }, "merged": merged, "draft": false,
        "base": { "ref": "main" },
        "head": { "sha": head, "ref": "topic", "repo": { "full_name": "fork/r" } },
        "user": { "login": "author", "id": 1 },
        "labels": [{ "name": "herald:merge" }],
        "title": "Topic", "body": "Why.",
        "mergeable": mergeable, "mergeable_state": state
    })
}

fn open(head: &str) -> Response {
    answer(200, &own_pull(head, (Some(true), "clean"), false))
}

fn checks(head: &str) -> (String, String) {
    (
        format!("GET https://api.github.com/repos/o/r/commits/{head}/check-runs?per_page=100"),
        format!("GET https://api.github.com/repos/o/r/commits/{head}/status?per_page=100"),
    )
}

fn commit(head: &str, (verified, email): (bool, &str), parents: &[&str]) -> (String, Value) {
    (
        format!("GET https://api.github.com/repos/o/r/commits/{head}"),
        json!({
            "sha": head,
            "parents": parents.iter().map(|sha| json!({ "sha": sha })).collect::<Vec<_>>(),
            "commit": { "committer": { "email": email }, "verification": { "verified": verified } }
        }),
    )
}

/// GitHub with pull request 7, head `head`, queued by `w`'s label on `h7`
/// and pushed since from each of `pushed`, its checks green, `behind`
/// commits behind base `b1`.
fn github(head: &str, pushed: &[&str], behind: u64) -> Forge {
    let mut activity: Vec<Value> = pushed
        .iter()
        .map(|before| json!({ "before": before, "timestamp": "2026-11-01T11:30:00Z", "activity_type": "push" }))
        .collect();
    activity.push(
        json!({ "before": "h6", "timestamp": "2026-11-01T10:00:00Z", "activity_type": "push" }),
    );
    let (runs, statuses) = checks(head);
    Forge::default()
        .answering(
            "GET https://api.github.com/repos/o/r",
            &[answer(200, &json!({ "private": false, "allow_merge_commit": true, "allow_squash_merge": true, "allow_rebase_merge": false }))],
        )
        .answering(
            "GET https://api.github.com/repos/o/r/branches/main",
            &[answer(200, &json!({ "commit": { "sha": "b1" }, "protected": false }))],
        )
        .answering(
            "GET https://api.github.com/repos/o/r/rules/branches/main?per_page=100&page=1",
            &[answer(200, &json!([]))],
        )
        .answering(
            "GET https://api.github.com/repos/o/r/pulls?state=open&base=main&per_page=100&page=1",
            &[answer(200, &json!([own_pull(head, (Some(true), "clean"), false)]))],
        )
        .answering(PULL, &[open(head)])
        .answering(EVENTS, &[answer(200, &json!([labeled()]))])
        .answering(PERMISSION, &[writer(true)])
        .answering(ACTIVITY, &[answer(200, &json!(activity))])
        .answering(
            &format!("GET https://api.github.com/repos/o/r/compare/b1...{head}?per_page=1"),
            &[answer(200, &json!({ "behind_by": behind }))],
        )
        .answering(
            &runs,
            &[answer(200, &json!({ "total_count": 1, "check_runs": [{ "name": "ci", "status": "completed", "conclusion": "success" }] }))],
        )
        .answering(&statuses, &[answer(200, &json!({ "total_count": 0, "statuses": [] }))])
        .answering(MERGE, &[answer(200, &json!({ "merged": true }))])
        .answering(UPDATE, &[answer(202, &json!({ "message": "Updating pull request branch." }))])
}

fn applied(extra: &str, (store, forge): (&Failing, &Forge), apply: bool) -> Vec<Next> {
    run_config(
        &format!("{OWN}{extra}"),
        (store, forge, &the_secrets()),
        (apply, Budget::UNBOUNDED),
    )
}

fn steps(store: &Failing) -> Vec<(StepKind, String, String, bool, Stepped)> {
    record(store)
        .map(|record| record.own.steps)
        .unwrap_or_default()
        .into_iter()
        .map(|step| (step.kind, step.head, step.base, step.clean, step.outcome))
        .collect()
}

fn left(store: &Failing) -> Option<Left> {
    record(store).and_then(|record| record.own.left.map(|leaving| leaving.left))
}

fn kept_step(store: &Failing, step: Step) {
    kept_on(store, step, GITHUB_API);
}

/// `step` recorded for `w`'s gesture on the forge whose API is `api`.
fn kept_on(store: &Failing, step: Step, api: &str) {
    let record = GestureRecord {
        key: GestureKey {
            api: api.to_owned(),
            ..key()
        },
        by: "w".to_owned(),
        at: gestured_at(),
        queue: "app".to_owned(),
        seen: "h7".to_owned(),
        seen_at: NOON,
        hand_off: None,
        taken_out: Vec::new(),
        own: Own {
            steps: vec![step],
            left: None,
        },
    };
    pollster::block_on(store.inner.put_gesture(&record, None)).unwrap();
}

/// A step of `kind` on `head` and `base` that came to `outcome`; an
/// update recorded as having made `h8`.
fn taken(kind: StepKind, (head, base): (&str, &str), clean: bool, outcome: Stepped) -> Step {
    Step {
        kind,
        head: head.to_owned(),
        base: base.to_owned(),
        clean,
        made: (kind == StepKind::Update).then(|| "h8".to_owned()),
        check: None,
        credential: "HERALD_MERGE".to_owned(),
        at: NOON,
        outcome,
    }
}

/// `forge` answering `line` with `now` `before` times, then with `then`.
fn after(forge: Forge, line: &str, (now, before): (Response, usize), then: Response) -> Forge {
    let mut answers = vec![now; before];
    answers.push(then);
    forge.answering(line, &answers)
}

/// How often a run that merges reads `line`, as `forge` was sent it.
fn reads(forge: &Forge, line: &str) -> usize {
    forge.lines().iter().filter(|sent| *sent == line).count()
}

#[test]
fn the_dry_run_reads_alone_and_the_apply_merges_with_the_head_it_proved() {
    let store = healthy();
    let forge = github("h7", &[], 0);
    assert_eq!(
        applied("", (&store, &forge), false),
        [Next::Verdict(Verdict::Merge)]
    );
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    assert!(!forge.lines().iter().any(|line| line.starts_with("PUT")));
    assert_eq!(record(&store), None);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))]
    );
    assert_eq!(forge.carrying("write-value"), [MERGE]);
    let body = &forge.bodies(MERGE)[0];
    assert_eq!(body["sha"], "h7");
    assert_eq!(body["merge_method"], "squash");
    assert_eq!(body.get("commit_title"), None);
    assert_eq!(
        steps(&store),
        [(
            StepKind::Merge,
            "h7".to_owned(),
            "b1".to_owned(),
            true,
            Stepped::Took
        )]
    );
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Held(taken(
            StepKind::Merge,
            ("h7", "b1"),
            true,
            Stepped::Took
        ))]
    );
    assert_eq!(forge.carrying("write-value"), [MERGE]);
}

#[test]
fn message_pull_merges_with_the_pull_request_s_title_and_body() {
    let store = healthy();
    let forge = github("h7", &[], 0);
    assert_eq!(
        applied("message = \"pull\"\n", (&store, &forge), true),
        [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))]
    );
    let body = &forge.bodies(MERGE)[0];
    assert_eq!(body["commit_title"], "Topic");
    assert_eq!(body["commit_message"], "Why.");
}

/// What the run after herald's update to `h8` from `h7` on `b1`, recorded
/// as `step`, comes to where GitHub says it made `h8` as `made` with
/// `parents`, under the queue's `extra` settings: each next step, why it
/// left, and each request carrying the write credential.
fn carried(
    step: Option<Step>,
    (made, parents): ((bool, &str), &[&str]),
    extra: &str,
) -> (Vec<Next>, Option<Left>, Vec<String>) {
    let store = healthy();
    if let Some(step) = step {
        kept_step(&store, step);
    }
    let (line, commit) = commit("h8", made, parents);
    let forge = github("h8", &["h7"], 0).answering(&line, &[answer(200, &commit)]);
    let nexts = applied(extra, (&store, &forge), true);
    (nexts, left(&store), forge.carrying("write-value"))
}

/// A recorded update, how GitHub made the new head, its parents, and what
/// the case is named.
type CarryCase<'a> = (Option<Step>, (bool, &'a str), &'a [&'a str], &'a str);

#[test]
fn each_condition_of_the_carry_failing_alone_carries_nothing_and_the_pull_request_leaves() {
    let update = |clean, outcome| Some(taken(StepKind::Update, ("h7", "b1"), clean, outcome));
    let signed = (true, "noreply@github.com");
    let (nexts, gone, writes) = carried(update(true, Stepped::Took), (signed, &["h7", "b1"]), "");
    assert_eq!(
        nexts,
        [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))]
    );
    assert_eq!((gone, writes), (None, vec![MERGE.to_owned()]));
    let failing: [CarryCase; 10] = [
        (None, signed, &["h7", "b1"], "no step recorded"),
        (
            update(false, Stepped::Took),
            signed,
            &["h7", "b1"],
            "a step that read a conflict",
        ),
        (
            update(true, Stepped::Refused),
            signed,
            &["h7", "b1"],
            "a step refused",
        ),
        (
            update(true, Stepped::Unsent),
            signed,
            &["h7", "b1"],
            "a step never sent",
        ),
        (
            update(true, Stepped::Took),
            signed,
            &["b1", "h7"],
            "parents in the other order",
        ),
        (
            update(true, Stepped::Took),
            signed,
            &["h8x"],
            "a descendant of the merge",
        ),
        (
            update(true, Stepped::Took),
            (false, "noreply@github.com"),
            &["h7", "b1"],
            "unsigned",
        ),
        (
            update(true, Stepped::Took),
            (true, "someone@example.com"),
            &["h7", "b1"],
            "another signer",
        ),
        (
            update(true, Stepped::Took),
            signed,
            &["h7", "b9"],
            "another base",
        ),
        (
            Some(taken(StepKind::Merge, ("h7", "b1"), true, Stepped::Took)),
            signed,
            &["h7", "b1"],
            "a merge, not an update",
        ),
    ];
    for (step, made, parents, case) in failing {
        let (nexts, gone, writes) = carried(step, (made, parents), "");
        assert_eq!(nexts, [Next::Acted(Acted::Left(Left::HeadMoved))], "{case}");
        assert_eq!(gone, Some(Left::HeadMoved), "{case}");
        assert_eq!(writes, Vec::<String>::new(), "{case}");
    }
    let (nexts, gone, _) = carried(
        update(true, Stepped::Took),
        (signed, &["h7", "b1"]),
        "updated = \"never\"\n",
    );
    assert_eq!(
        (nexts, gone),
        (
            vec![Next::Acted(Acted::Left(Left::HeadMoved))],
            Some(Left::HeadMoved)
        )
    );
}

/// What the run after a recorded update to `h8` comes to on the GitHub
/// server at `api`, the queue naming `signer` where it does, where `h8` is
/// signed for `email`.
fn on_server(api: &str, signer: Option<&str>, email: &str) -> Vec<Next> {
    let server = format!(
        "[sources.app]\nkind = \"github\"\nrepository = \"o/r\"\napi = \"{api}\"\ntoken = {{ secret = \"HERALD_READ\" }}\n[merge_queues.app]\nsource = \"app\"\nbranch = \"main\"\nruns = \"herald\"\nmethod = \"squash\"\nwrite = {{ token = {{ secret = \"HERALD_MERGE\" }} }}\n{}",
        signer
            .map(|signer| format!("signer = \"{signer}\"\n"))
            .unwrap_or_default()
    );
    let store = healthy();
    kept_on(
        &store,
        taken(StepKind::Update, ("h7", "b1"), true, Stepped::Took),
        api,
    );
    let (line, made) = commit("h8", (true, email), &["h7", "b1"]);
    let forge = rebased(
        &github("h8", &["h7"], 0).answering(&line, &[answer(200, &made)]),
        api,
    );
    run_on(
        &server,
        (&store, &forge, &the_secrets()),
        (true, Budget::UNBOUNDED),
        api,
    )
}

/// `forge` answering on the API `api` what it answers on github.com.
fn rebased(forge: &Forge, api: &str) -> Forge {
    let answers = forge
        .answers
        .lock()
        .unwrap()
        .iter()
        .map(|(line, answers)| (line.replace("https://api.github.com", api), answers.clone()))
        .collect();
    Forge {
        answers: Arc::new(Mutex::new(answers)),
        ..Forge::default()
    }
}

#[test]
fn on_a_github_server_only_the_signer_named_carries_and_none_without_one() {
    for api in ["https://ghe.example/api/v3", "https://api.acme.ghe.com"] {
        assert_eq!(
            on_server(api, None, "noreply@github.com"),
            [Next::Acted(Acted::Left(Left::HeadMoved))],
            "{api}"
        );
        assert_eq!(
            on_server(api, Some("web-flow@ghe.example"), "web-flow@ghe.example"),
            [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))],
            "{api}"
        );
        assert_eq!(
            on_server(api, Some("web-flow@ghe.example"), "noreply@github.com"),
            [Next::Acted(Acted::Left(Left::HeadMoved))],
            "{api}"
        );
    }
}

#[test]
fn a_conflict_read_leaves_and_a_conflict_github_still_works_out_waits() {
    let store = healthy();
    let dirty = answer(200, &own_pull("h7", (Some(false), "dirty"), false));
    let forge = github("h7", &[], 2).answering(PULL, &[dirty]);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Left(Left::Conflict))]
    );
    assert_eq!(left(&store), Some(Left::Conflict));
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    let store = healthy();
    let unknown = answer(200, &own_pull("h7", (None, "unknown"), false));
    let forge = github("h7", &[], 2).answering(PULL, &[unknown]);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Verdict(Verdict::Wait)]
    );
    assert_eq!(record(&store), None);
}

#[test]
fn a_head_moved_a_role_lost_a_cancel_or_a_draft_since_the_queue_s_read_takes_no_step() {
    let probe = github("h7", &[], 0);
    applied("", (&healthy(), &probe), false);
    let pulls = reads(&probe, PULL);
    let events = reads(&probe, EVENTS);
    let roles = reads(&probe, PERMISSION);
    let moved = after(github("h7", &[], 0), PULL, (open("h7"), pulls), open("h9"));
    let unroled = after(
        github("h7", &[], 0),
        PERMISSION,
        (writer(true), roles),
        writer(false),
    );
    let ghost = after(
        github("h7", &[], 0),
        PERMISSION,
        (writer(true), roles),
        answer(404, &json!({})),
    );
    let unsaid = after(
        github("h7", &[], 0),
        PERMISSION,
        (writer(true), roles),
        answer(200, &json!({})),
    );
    let cancelled = after(
        github("h7", &[], 0),
        EVENTS,
        (answer(200, &json!([labeled()])), events),
        answer(200, &json!([labeled(), label("unlabeled", 502, "11:40")])),
    );
    let mut drafted = own_pull("h7", (Some(true), "clean"), false);
    drafted["draft"] = json!(true);
    let drafted = after(
        github("h7", &[], 0),
        PULL,
        (open("h7"), pulls),
        answer(200, &drafted),
    );
    for (forge, next, gone) in [
        (
            moved,
            Next::Acted(Acted::Left(Left::HeadMoved)),
            Some(Left::HeadMoved),
        ),
        (
            unroled,
            Next::Acted(Acted::Left(Left::Removed)),
            Some(Left::Removed),
        ),
        (
            ghost,
            Next::Acted(Acted::Left(Left::Removed)),
            Some(Left::Removed),
        ),
        (
            unsaid,
            Next::Acted(Acted::Refused(OWN_UNSAID.to_owned())),
            None,
        ),
        (
            cancelled,
            Next::Acted(Acted::Refused(GONE.to_owned())),
            None,
        ),
        (
            drafted,
            Next::Acted(Acted::Refused(OWN_DRAFTED.to_owned())),
            None,
        ),
    ] {
        let store = healthy();
        assert_eq!(
            applied("", (&store, &forge), true),
            std::slice::from_ref(&next),
            "{next:?}"
        );
        assert_eq!(left(&store), gone, "{next:?}");
        assert_eq!(steps(&store), [], "{next:?}");
        assert_eq!(
            forge.carrying("write-value"),
            Vec::<String>::new(),
            "{next:?}"
        );
    }
}

#[test]
fn a_fresh_read_due_for_another_step_or_a_method_the_repository_refuses_takes_none() {
    let probe = github("h7", &[], 0);
    applied("", (&healthy(), &probe), true);
    let compares = reads(
        &probe,
        "GET https://api.github.com/repos/o/r/compare/b1...h7?per_page=1",
    ) - 1;
    let behind = after(
        github("h7", &[], 0),
        "GET https://api.github.com/repos/o/r/compare/b1...h7?per_page=1",
        (answer(200, &json!({ "behind_by": 0 })), compares),
        answer(200, &json!({ "behind_by": 3 })),
    );
    let store = healthy();
    assert_eq!(
        applied("", (&store, &behind), true),
        [Next::Acted(Acted::Refused(UNDUE.to_owned()))]
    );
    assert_eq!(record(&store), None);
    let refusing = github("h7", &[], 0).answering(
        "GET https://api.github.com/repos/o/r",
        &[answer(200, &json!({ "allow_merge_commit": true, "allow_squash_merge": false, "allow_rebase_merge": false }))],
    );
    let store = healthy();
    let refused = applied("", (&store, &refusing), true);
    assert!(
        matches!(&refused[..], [Next::Acted(Acted::Refused(why))] if why.contains("does not allow a merge by \"squash\"")),
        "{refused:?}"
    );
    let unsaid = github("h7", &[], 0).answering(
        "GET https://api.github.com/repos/o/r",
        &[answer(200, &json!({ "allow_squash_merge": true }))],
    );
    assert_eq!(
        applied("", (&healthy(), &unsaid), true),
        [Next::Acted(Acted::Refused(METHODS_UNREAD.to_owned()))]
    );
    for forge in [refusing, unsaid] {
        assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    }
}

#[test]
fn a_duplicate_run_a_record_unwritten_or_one_unsettled_takes_no_second_step() {
    let store = Failing {
        fault: Fault::ActionsConflict,
        ..healthy()
    };
    let forge = github("h7", &[], 0);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Refused(RACED.to_owned()))]
    );
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    let store = Failing {
        fault: Fault::WritesActions,
        ..healthy()
    };
    let forge = github("h7", &[], 0);
    let refused = applied("", (&store, &forge), true);
    assert!(
        matches!(&refused[..], [Next::Acted(Acted::Refused(why))] if why.contains("could not be written")),
        "{refused:?}"
    );
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    let store = Failing {
        fault: Fault::SettlesNoAction,
        ..healthy()
    };
    let forge = github("h7", &[], 0);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Unrecorded(
            StepKind::Merge,
            Stepped::Took
        ))]
    );
    assert_eq!(forge.carrying("write-value"), [MERGE]);
}

#[test]
fn a_step_recorded_before_a_crash_is_never_taken_again_and_one_never_sent_is() {
    for outcome in [Stepped::Calling, Stepped::Unsettled] {
        let store = healthy();
        let step = taken(StepKind::Merge, ("h7", "b1"), true, outcome);
        kept_step(&store, step.clone());
        let forge = github("h7", &[], 0);
        assert_eq!(
            applied("", (&store, &forge), true),
            [Next::Held(step.clone())]
        );
        assert_eq!(applied("", (&store, &forge), false), [Next::Held(step)]);
        assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
        let store = healthy();
        let step = taken(StepKind::Update, ("h7", "b1"), true, outcome);
        kept_step(&store, step.clone());
        let failed = json!({ "total_count": 1, "check_runs": [{ "name": "ci", "status": "completed", "conclusion": "failure" }] });
        let forge = github("h7", &[], 0).answering(&checks("h7").0, &[answer(200, &failed)]);
        assert_eq!(applied("", (&store, &forge), true), [Next::Held(step)]);
        assert_eq!(left(&store), None);
    }
    let store = healthy();
    kept_step(
        &store,
        taken(StepKind::Merge, ("h7", "b1"), true, Stepped::Unsent),
    );
    let forge = github("h7", &[], 0);
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))]
    );
    assert_eq!(forge.carrying("write-value"), [MERGE]);
}

#[test]
fn a_lost_answer_is_read_back_and_never_asked_again() {
    let probe = github("h7", &[], 0);
    applied("", (&healthy(), &probe), true);
    let pulls = reads(&probe, PULL);
    let lost = answer(502, &json!({}));
    for (merged, outcome) in [(true, Stepped::Took), (false, Stepped::Unsettled)] {
        let back = answer(200, &own_pull("h7", (Some(true), "clean"), merged));
        let forge = after(github("h7", &[], 0), PULL, (open("h7"), pulls), back)
            .answering(MERGE, std::slice::from_ref(&lost));
        let store = healthy();
        assert_eq!(
            applied("", (&store, &forge), true),
            [Next::Acted(Acted::Stepped(StepKind::Merge, outcome))]
        );
        assert_eq!(forge.carrying("write-value"), [MERGE]);
        assert_eq!(reads(&forge, PULL), pulls + 1);
        let again = applied("", (&store, &forge), true);
        assert!(matches!(&again[..], [Next::Held(_)]), "{again:?}");
        assert_eq!(forge.carrying("write-value"), [MERGE]);
    }
    let probe = github("h7", &[], 2).answering(UPDATE, std::slice::from_ref(&lost));
    applied("", (&healthy(), &probe), true);
    let pulls = reads(&probe, PULL);
    let (line, made) = commit("h8", (true, "noreply@github.com"), &["h7", "b1"]);
    for (head, outcome, kept) in [
        ("h8", Stepped::Took, Some("h8")),
        ("h7", Stepped::Unsettled, None),
    ] {
        let forge = after(
            github("h7", &[], 2),
            PULL,
            (open("h7"), pulls - 1),
            open(head),
        )
        .answering(UPDATE, std::slice::from_ref(&lost))
        .answering(&line, &[answer(200, &made)]);
        let store = healthy();
        assert_eq!(
            applied("", (&store, &forge), true),
            [Next::Acted(Acted::Stepped(StepKind::Update, outcome))],
            "{head}"
        );
        assert_eq!(reads(&forge, PULL), pulls, "{head}");
        assert_eq!(
            record(&store).and_then(|record| record.own.steps[0].made.clone()),
            kept.map(ToOwned::to_owned),
            "{head}"
        );
    }
}

#[test]
fn a_refusal_or_a_conflict_leaves_and_a_refusal_on_a_moved_head_says_so() {
    let probe = github("h7", &[], 0);
    applied("", (&healthy(), &probe), true);
    let pulls = reads(&probe, PULL);
    let refused = answer(405, &json!({ "message": "Base branch was modified" }));
    for (head, outcome, gone) in [
        ("h7", Stepped::Refused, Left::MergeRefused),
        ("h9", Stepped::HeadMoved, Left::HeadMoved),
    ] {
        let forge = after(github("h7", &[], 0), PULL, (open("h7"), pulls), open(head))
            .answering(MERGE, std::slice::from_ref(&refused));
        let store = healthy();
        assert_eq!(
            applied("", (&store, &forge), true),
            [Next::Acted(Acted::Stepped(StepKind::Merge, outcome))]
        );
        assert_eq!(left(&store), Some(gone));
        assert_eq!(
            applied("", (&store, &forge), true),
            [Next::Verdict(Verdict::Leaves(gone))]
        );
        assert_eq!(forge.carrying("write-value"), [MERGE]);
    }
    let conflicting = answer(
        422,
        &json!({ "message": "merge conflict between base and head" }),
    );
    let forge = github("h7", &[], 2).answering(UPDATE, &[conflicting]);
    let store = healthy();
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Stepped(
            StepKind::Update,
            Stepped::Conflict
        ))]
    );
    assert_eq!(left(&store), Some(Left::Conflict));
}

#[test]
fn a_call_the_budget_never_sent_is_recorded_unsent_and_taken_on_the_next_run() {
    let store = healthy();
    let forge = github("h7", &[], 0);
    let budget = Budget {
        until: Some(NOON),
        ..Budget::UNBOUNDED
    };
    let nexts = run_config(OWN, (&store, &forge, &the_secrets()), (true, budget));
    assert_eq!(
        nexts,
        [Next::Acted(Acted::Stepped(
            StepKind::Merge,
            Stepped::Unsent
        ))]
    );
    assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
    assert_eq!(
        applied("", (&store, &forge), true),
        [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))]
    );
}
