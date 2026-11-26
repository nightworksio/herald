//! herald's own queue on GitLab and Forgejo: neither takes the head a
//! branch update must find, so it is read again right before the call;
//! each merge names the head it proved; and no update there ever carries a
//! gesture to the head it made.

use herald_core::merging::{GestureKey, GestureRecord, Left, Own, Step, StepKind, Stepped};
use herald_flows::{Acted, Budget, Next};
use herald_ports::Actions as _;
use serde_json::{Value, json};

use super::{Forge, answer, gestured_at, run_on, the_secrets};
use crate::harness::{Failing, NOON, healthy};

const GITLAB: &str = "https://gitlab.com/api/v4";
const PROJECT: &str = "https://gitlab.com/api/v4/projects/o%2Fr";
const FORGEJO: &str = "https://codeberg.org/api/v1";
const REPOSITORY: &str = "https://codeberg.org/api/v1/repos/o/r";

/// One forge's own queue as the tests drive it: its API, its queue's
/// settings, the line its pull request is read again by, the update's and
/// the merge's lines, and the forge answering with the pull request's head
/// and how far it is behind its base.
struct Kind {
    api: &'static str,
    queued: String,
    again: String,
    update: String,
    merge: String,
    settings: String,
    forge: fn(&str, u64) -> Forge,
}

fn queued(kind: &str, api: &str) -> String {
    format!(
        "[sources.app]\nkind = \"{kind}\"\nrepository = \"o/r\"\napi = \"{api}\"\ntoken = {{ secret = \"HERALD_READ\" }}\n[merge_queues.app]\nsource = \"app\"\nbranch = \"main\"\nruns = \"herald\"\nmethod = \"merge\"\nwrite = {{ token = {{ secret = \"HERALD_MERGE\" }} }}\n"
    )
}

fn merge_request(head: &str) -> Value {
    json!({
        "iid": 7, "web_url": "https://gitlab.com/o/r/-/merge_requests/7", "draft": false,
        "sha": head, "author": { "id": 1, "username": "author" }, "labels": ["herald:merge"],
        "source_branch": "topic", "target_branch": "main", "state": "opened",
        "title": "Topic", "description": "Why."
    })
}

/// GitLab with merge request 7, head `head`, `behind` commits behind its
/// base, queued by `w`'s label on the version `h7`.
fn gitlab(head: &str, behind: u64) -> Forge {
    let mut versions =
        vec![json!({ "head_commit_sha": "h7", "created_at": "2026-11-01T10:00:00Z" })];
    if head != "h7" {
        versions.insert(
            0,
            json!({ "head_commit_sha": head, "created_at": "2026-11-01T11:30:00Z" }),
        );
    }
    Forge::default()
        .answering(
            &format!("GET {PROJECT}"),
            &[answer(200, &json!({ "visibility": "public", "merge_trains_enabled": false,
                "only_allow_merge_if_pipeline_succeeds": true, "merge_method": "merge", "squash_option": "default_off" }))],
        )
        .answering(
            &format!("GET {PROJECT}/repository/branches/main"),
            &[answer(200, &json!({ "commit": { "id": "b1" } }))],
        )
        .answering(
            &format!("GET {PROJECT}/merge_requests?state=opened&target_branch=main&per_page=100&page=1"),
            &[answer(200, &json!([merge_request(head)]))],
        )
        .answering(&format!("GET {PROJECT}/merge_requests/7"), &[answer(200, &merge_request(head))])
        .answering(
            &format!("GET {PROJECT}/merge_requests/7/resource_label_events?per_page=100&page=1"),
            &[answer(200, &json!([{ "id": 501, "user": { "id": 2, "username": "w" },
                "created_at": "2026-11-01T11:00:00Z", "label": { "name": "herald:merge" }, "action": "add" }]))],
        )
        .answering(&format!("GET {PROJECT}/members/all/2"), &[answer(200, &json!({ "access_level": 30 }))])
        .answering(
            &format!("GET {PROJECT}/merge_requests/7/versions?per_page=100"),
            &[answer(200, &json!(versions))],
        )
        .answering(
            &format!("GET {PROJECT}/merge_requests/7?include_diverged_commits_count=true"),
            &[answer(200, &json!({ "detailed_merge_status": "mergeable", "diverged_commits_count": behind,
                "has_conflicts": false, "head_pipeline": { "id": 9, "sha": head, "status": "success" } }))],
        )
        .answering(
            &format!("GET {PROJECT}/repository/commits/{head}/statuses?per_page=100&page=1"),
            &[answer(200, &json!([{ "id": 90, "name": "test", "status": "success" }]))],
        )
        .answering(
            &format!("GET {PROJECT}/repository/commits/{head}/statuses?pipeline_id=9&per_page=100&page=1"),
            &[answer(200, &json!([{ "id": 90, "name": "test", "status": "success" }]))],
        )
        .answering(&format!("PUT {PROJECT}/merge_requests/7/rebase"), &[answer(202, &json!({ "rebase_in_progress": true }))])
        .answering(&format!("PUT {PROJECT}/merge_requests/7/merge"), &[answer(200, &json!({ "state": "merged" }))])
}

fn pull(head: &str, base: &str) -> Value {
    json!({
        "number": 7, "html_url": "https://codeberg.org/o/r/pulls/7", "state": "open", "draft": false,
        "head": { "sha": head, "ref": "topic", "repo": { "full_name": "o/r" } },
        "base": { "ref": "main" }, "user": { "login": "author" },
        "labels": [{ "name": "herald:merge" }], "title": "Topic", "body": "Why.",
        "merge_base": base, "mergeable": true
    })
}

/// Forgejo with pull request 7, head `head`, `behind` its base when not 0,
/// queued by `w`'s label on `h7` and force-pushed since where `head` is
/// another.
fn forgejo(head: &str, behind: u64) -> Forge {
    let base = if behind == 0 { "b1" } else { "b0" };
    let mut timeline = vec![
        json!({ "id": 501, "type": "label", "user": { "login": "w" },
        "created_at": "2026-11-01T11:00:00Z", "body": "1", "label": { "name": "herald:merge" } }),
    ];
    if head != "h7" {
        let pushed = json!({ "is_force_push": true, "commit_ids": ["h7", head] }).to_string();
        timeline.push(json!({ "type": "pull_push", "user": { "login": "author" },
            "created_at": "2026-11-01T11:30:00Z", "body": pushed }));
    }
    Forge::default()
        .answering(
            &format!("GET {REPOSITORY}"),
            &[answer(200, &json!({ "private": false, "internal": false, "allow_merge_commits": true,
                "allow_squash_merge": true, "allow_rebase": true, "allow_fast_forward_only_merge": true }))],
        )
        .answering(&format!("GET {REPOSITORY}/branches/main"), &[answer(200, &json!({ "commit": { "id": "b1" } }))])
        .answering(&format!("GET {REPOSITORY}/pulls?state=open&limit=50&page=1"), &[answer(200, &json!([pull(head, base)]))])
        .answering(&format!("GET {REPOSITORY}/pulls?state=open&limit=50&page=2"), &[answer(200, &json!([]))])
        .answering(&format!("GET {REPOSITORY}/pulls/7"), &[answer(200, &pull(head, base))])
        .answering(&format!("GET {REPOSITORY}/issues/7/timeline?limit=50&page=2"), &[answer(200, &json!([]))])
        .answering(
            &format!("GET {REPOSITORY}/issues/7/timeline?limit=50&page=1"),
            &[answer(200, &json!(timeline))],
        )
        .answering(&format!("GET {REPOSITORY}/collaborators/w/permission"), &[answer(200, &json!({ "permission": "write" }))])
        .answering(
            &format!("GET {REPOSITORY}/commits/{head}/status?limit=50"),
            &[answer(200, &json!({ "total_count": 1, "statuses": [{ "context": "ci", "status": "success" }] }))],
        )
        .answering(&format!("POST {REPOSITORY}/pulls/7/update?style=merge"), &[answer(200, &json!({}))])
        .answering(&format!("POST {REPOSITORY}/pulls/7/merge"), &[answer(200, &json!({}))])
}

fn kinds() -> [Kind; 2] {
    [
        Kind {
            api: GITLAB,
            queued: queued("gitlab", GITLAB),
            again: format!("GET {PROJECT}/merge_requests/7"),
            update: format!("PUT {PROJECT}/merge_requests/7/rebase"),
            merge: format!("PUT {PROJECT}/merge_requests/7/merge"),
            settings: format!("GET {PROJECT}"),
            forge: gitlab,
        },
        Kind {
            api: FORGEJO,
            queued: queued("forgejo", FORGEJO),
            again: format!("GET {REPOSITORY}/pulls/7"),
            update: format!("POST {REPOSITORY}/pulls/7/update?style=merge"),
            merge: format!("POST {REPOSITORY}/pulls/7/merge"),
            settings: format!("GET {REPOSITORY}"),
            forge: forgejo,
        },
    ]
}

fn applied(kind: &Kind, (store, forge): (&Failing, &Forge)) -> Vec<Next> {
    run_on(
        &kind.queued,
        (store, forge, &the_secrets()),
        (true, Budget::UNBOUNDED),
        kind.api,
    )
}

/// `forge` answering `line` with its own answer `before` times, then with
/// `then`'s.
fn after(forge: Forge, line: &str, before: usize, then: &Forge) -> Forge {
    let now = forge.answers.lock().unwrap()[line][0].clone();
    let later = then.answers.lock().unwrap()[line][0].clone();
    let mut answers = vec![now; before];
    answers.push(later);
    forge.answering(line, &answers)
}

/// How often `forge` was sent `line` before it was first sent `until`.
fn reads_before(forge: &Forge, (line, until): (&str, &str)) -> usize {
    forge
        .lines()
        .iter()
        .take_while(|sent| *sent != until)
        .filter(|sent| *sent == line)
        .count()
}

#[test]
fn an_update_is_read_again_right_before_the_call_and_never_sent_on_a_moved_head() {
    for kind in kinds() {
        let forge = (kind.forge)("h7", 2);
        assert_eq!(
            applied(&kind, (&healthy(), &forge)),
            [Next::Acted(Acted::Stepped(StepKind::Update, Stepped::Took))],
            "{}",
            kind.api
        );
        assert_eq!(
            forge.carrying("write-value"),
            std::slice::from_ref(&kind.update)
        );
        let right_before = 1;
        let before = reads_before(&forge, (&kind.again, &kind.update)) - right_before;
        let moved = after(
            (kind.forge)("h7", 2),
            &kind.again,
            before,
            &(kind.forge)("h9", 2),
        );
        assert_eq!(
            applied(&kind, (&healthy(), &moved)),
            [Next::Acted(Acted::Stepped(
                StepKind::Update,
                Stepped::Unsent
            ))],
            "{}",
            kind.api
        );
        assert_eq!(moved.carrying("write-value"), Vec::<String>::new());
    }
}

#[test]
fn a_merge_names_the_head_it_proved() {
    for (kind, field) in kinds().into_iter().zip(["sha", "head_commit_id"]) {
        let forge = (kind.forge)("h7", 0);
        assert_eq!(
            applied(&kind, (&healthy(), &forge)),
            [Next::Acted(Acted::Stepped(StepKind::Merge, Stepped::Took))],
            "{:?}",
            forge.lines()
        );
        assert_eq!(
            forge.carrying("write-value"),
            std::slice::from_ref(&kind.merge)
        );
        assert_eq!(forge.bodies(&kind.merge)[0][field], "h7", "{}", kind.api);
        let lines = forge.lines();
        let read = lines.iter().rposition(|line| *line == kind.settings);
        assert!(!lines[read.unwrap()..].contains(&kind.again), "{lines:?}");
    }
}

#[test]
fn an_update_herald_made_never_carries_the_gesture_to_its_head() {
    for kind in kinds() {
        let store = healthy();
        let record = GestureRecord {
            key: GestureKey {
                api: kind.api.to_owned(),
                repository: "o/r".to_owned(),
                number: 7,
                gesture: 501,
            },
            by: "w".to_owned(),
            at: gestured_at(),
            queue: "app".to_owned(),
            seen: "h7".to_owned(),
            seen_at: NOON,
            hand_off: None,
            taken_out: Vec::new(),
            own: Own {
                steps: vec![Step {
                    kind: StepKind::Update,
                    head: "h7".to_owned(),
                    base: "b1".to_owned(),
                    clean: true,
                    made: None,
                    check: None,
                    credential: "HERALD_MERGE".to_owned(),
                    at: NOON,
                    outcome: Stepped::Took,
                }],
                left: None,
            },
        };
        pollster::block_on(store.inner.put_gesture(&record, None)).unwrap();
        let forge = (kind.forge)("h8", 0);
        assert_eq!(
            applied(&kind, (&store, &forge)),
            [Next::Acted(Acted::Left(Left::HeadMoved))],
            "{}",
            kind.api
        );
        assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
        assert!(
            !forge
                .lines()
                .iter()
                .any(|line| line.contains("/commits/h8")),
            "{:?}",
            forge.lines()
        );
    }
}
