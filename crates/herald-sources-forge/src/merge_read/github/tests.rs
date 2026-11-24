use herald_core::Permission;
use herald_core::merging::{Check, CheckState, Enter, Vouches};
use herald_ports::Response;
use serde_json::{Value, json};

use super::super::tests::{
    LABEL, approved_by, at, at_second, base, branched, entered, entered_least, entry, ok, pages,
    proved, refused, stamp,
};
use super::*;
use crate::Forge;

fn pull(head: &str) -> Value {
    json!([{
        "number": 1,
        "html_url": "https://github.com/o/r/pull/1",
        "draft": false,
        "head": {"sha": head, "ref": "feature", "repo": {"full_name": "fork/r"}},
        "user": {"login": "author", "id": 1},
        "labels": [{"name": LABEL}]
    }])
}

fn labeled(event: &str, login: &str, minute: u32) -> Value {
    json!({
        "event": event,
        "actor": {"login": login, "id": 2},
        "created_at": stamp(minute),
        "label": {"name": LABEL}
    })
}

fn writer() -> Value {
    json!({"permission": "write", "role_name": "write", "user": {"permissions": {"pull": true, "push": true}}})
}

fn pushed(before: &str, minute: u32) -> Value {
    json!({"before": before, "after": "x", "timestamp": stamp(minute), "activity_type": "push"})
}

#[test]
fn a_label_a_writer_added_stands_on_the_head_github_records_then() {
    let read = entered(
        Forge::GitHub,
        vec![
            ok(&pull("h2")),
            ok(&json!([labeled("labeled", "w", 10), {"event": "labeled", "actor": null, "created_at": stamp(11), "label": {"name": LABEL}}])),
            ok(&writer()),
            ok(&json!([pushed("h1", 5)])),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert_eq!(read.len(), 1);
    let entered = &read[0];
    assert_eq!(entered.gesture.by, "w");
    assert_eq!(entered.gesture.at, at(10));
    assert_eq!(entered.gesture.head.as_deref(), Some("h2"));
    assert_eq!(entered.gesture.role, Some(Permission::Write));
    assert_eq!(entered.arrived, Some(at(5)));
    assert!(entered.whole);
}

#[test]
fn a_push_after_the_label_leaves_the_gesture_on_the_head_it_was_made_on() {
    let read = entered(
        Forge::GitHub,
        vec![
            ok(&pull("h3")),
            ok(&json!([labeled("labeled", "w", 10)])),
            ok(&writer()),
            ok(&json!([pushed("h2", 20), pushed("h1", 5)])),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert_eq!(read[0].gesture.head.as_deref(), Some("h2"));
    assert_eq!(read[0].head, "h3");
}

#[test]
fn a_label_removed_by_a_deleted_account_still_takes_the_gesture_out() {
    let ghost = |event: &str, minute: u32| json!({"event": event, "actor": null, "created_at": stamp(minute), "label": {"name": LABEL}});
    let read = entered(
        Forge::GitHub,
        vec![
            ok(&pull("h2")),
            ok(&json!([
                labeled("labeled", "w", 10),
                ghost("unlabeled", 11),
                ghost("labeled", 12)
            ])),
            ok(&writer()),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert_eq!(read, []);
}

#[test]
fn a_label_removed_or_added_by_a_reader_queues_nothing() {
    let removed = entered(
        Forge::GitHub,
        vec![
            ok(&pull("h2")),
            ok(&json!([
                labeled("labeled", "w", 10),
                labeled("unlabeled", "x", 12)
            ])),
            ok(&writer()),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert_eq!(removed, []);
    let reader_only = json!({"user": {"permissions": {"pull": true, "triage": true}}});
    let triaged = entered(
        Forge::GitHub,
        vec![
            ok(&pull("h2")),
            ok(&json!([labeled("labeled", "t", 10)])),
            ok(&reader_only),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert_eq!(triaged, []);
}

#[test]
fn a_label_removed_past_the_events_herald_reads_is_never_read_as_standing() {
    let mut answers = vec![ok(&pull("h2"))];
    answers.extend(pages(5, 100, |index| {
        if index == 0 {
            labeled("labeled", "w", 10)
        } else {
            json!({"event": "mentioned", "actor": {"login": "m"}, "created_at": stamp(11)})
        }
    }));
    answers.push(ok(&writer()));
    answers.push(ok(&json!([])));
    let read = entered(Forge::GitHub, answers, &Enter::default()).unwrap();
    assert_eq!(read.len(), 1);
    assert!(!read[0].whole);
}

#[test]
fn a_comment_cancel_counts_from_its_author_a_writer_or_the_one_who_queued() {
    let enter = Enter::Comment("/merge".to_owned());
    let comment = |login: &str, body: &str, minute: u32| json!({"user": {"login": login}, "body": body, "created_at": stamp(minute), "updated_at": stamp(minute)});
    let stranger = entered(
        Forge::GitHub,
        vec![
            ok(&pull("h2")),
            ok(&json!([
                comment("w", "/merge", 10),
                comment("s", "/merge cancel", 11)
            ])),
            ok(&writer()),
            refused(404),
            ok(&json!([])),
        ],
        &enter,
    )
    .unwrap();
    assert_eq!(stranger.len(), 1);
    let by_author = entered(
        Forge::GitHub,
        vec![
            ok(&pull("h2")),
            ok(&json!([
                comment("w", "/merge", 10),
                comment("author", "/merge cancel", 11)
            ])),
            ok(&writer()),
        ],
        &enter,
    )
    .unwrap();
    assert_eq!(by_author, []);
}

#[test]
fn a_gesture_counts_only_from_one_who_holds_the_least_role_the_queue_asks() {
    let maintainer =
        json!({"user": {"permissions": {"pull": true, "push": true, "maintain": true}}});
    let asked = |role: &Value| {
        entered_least(
            Forge::GitHub,
            vec![
                ok(&pull("h2")),
                ok(&json!([labeled("labeled", "w", 10)])),
                ok(role),
                ok(&json!([])),
            ],
            (&Enter::default(), Permission::Maintain, Vouches::Gesture),
        )
        .unwrap()
    };
    assert_eq!(asked(&writer()), []);
    let held = asked(&maintainer);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].gesture.role, Some(Permission::Maintain));
}

#[test]
fn a_new_command_after_a_push_vouches_for_the_head_then() {
    let enter = Enter::Comment("/merge".to_owned());
    let comment = |minute: u32| json!({"user": {"login": "w"}, "body": "/merge", "created_at": stamp(minute), "updated_at": stamp(minute)});
    let read = entered(
        Forge::GitHub,
        vec![
            ok(&pull("h2")),
            ok(&json!([comment(10), comment(20)])),
            ok(&writer()),
            ok(&json!([pushed("h1", 15)])),
        ],
        &enter,
    )
    .unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].gesture.at, at(20));
    assert_eq!(read[0].gesture.head.as_deref(), Some("h2"));
}

#[test]
fn a_head_is_proved_from_its_comparison_merge_state_runs_and_statuses() {
    let answers = |runs_total: usize| {
        vec![
            ok(&json!({"behind_by": 0})),
            ok(&json!({"mergeable_state": "clean"})),
            ok(&json!({"total_count": runs_total, "check_runs": [
                {"name": "test", "status": "completed", "conclusion": "success"},
                {"name": "lint", "status": "in_progress", "conclusion": null},
                {"name": "old", "status": "completed", "conclusion": "stale"}
            ]})),
            ok(&json!({"total_count": 3, "statuses": [
                {"context": "ci/x", "state": "error"},
                {"context": "ci/y", "state": "pending"},
                {"context": "ci/z", "state": "success"}
            ]})),
        ]
    };
    let read = proved(
        Forge::GitHub,
        answers(3),
        (&entry("h2"), &base(&["test"], true)),
    )
    .unwrap();
    assert!(read.whole);
    assert!(read.up_to_date);
    assert_eq!(read.mergeable, Some(true));
    assert_eq!(read.required, ["test"]);
    let named = |name: &str, state| Check {
        name: name.to_owned(),
        state,
    };
    assert_eq!(
        read.checks,
        [
            named("test", CheckState::Passed),
            named("lint", CheckState::Pending),
            named("old", CheckState::Failed),
            named("ci/x", CheckState::Failed),
            named("ci/y", CheckState::Pending),
            named("ci/z", CheckState::Passed)
        ]
    );
    let partial = proved(Forge::GitHub, answers(4), (&entry("h2"), &base(&[], true))).unwrap();
    assert!(!partial.whole);
    let unread_base = proved(Forge::GitHub, answers(3), (&entry("h2"), &base(&[], false))).unwrap();
    assert!(!unread_base.whole);
}

#[test]
fn github_s_merge_states_say_whether_it_merges() {
    let states = [
        "clean",
        "has_hooks",
        "unstable",
        "dirty",
        "blocked",
        "draft",
        "behind",
        "unknown",
    ];
    let said: Vec<Option<bool>> = states.iter().map(|state| mergeable(Some(state))).collect();
    let (yes, no) = (Some(true), Some(false));
    assert_eq!(said, [yes, yes, yes, no, no, no, None, None]);
    assert_eq!(mergeable(None), None);
}

#[test]
fn a_branch_s_required_checks_come_from_its_protection_and_rulesets() {
    let answers = |protected: bool, rules: Value| {
        vec![
            ok(&json!({"visibility": "internal", "private": true})),
            ok(
                &json!({"commit": {"sha": "b1"}, "protected": protected, "protection": {
                    "required_status_checks": {"contexts": ["a"], "checks": [{"context": "b"}]}
                }}),
            ),
            ok(&rules),
        ]
    };
    let queued = json!([
        {"type": "merge_queue", "parameters": {"merge_method": "SQUASH"}},
        {"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "c"}, {"context": "a"}]}}
    ]);
    let read = branched(Forge::GitHub, answers(true, queued)).unwrap();
    assert_eq!(read.visibility, herald_core::Visibility::Internal);
    assert_eq!(read.head, "b1");
    assert_eq!(read.required, ["a", "b", "c"]);
    assert_eq!(read.queue, Some(true));
    assert!(read.whole);
    let classic = branched(Forge::GitHub, answers(true, json!([]))).unwrap();
    assert_eq!(classic.queue, None);
    let open = branched(Forge::GitHub, answers(false, json!([]))).unwrap();
    assert_eq!(open.queue, Some(false));
    let checked = json!([{"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "c"}]}}]);
    let unqueued = branched(Forge::GitHub, answers(false, checked)).unwrap();
    assert_eq!(unqueued.queue, Some(false));
}

fn reviewed(login: &str, state: &str, commit: &str) -> Value {
    json!({"user": {"login": login, "id": 3}, "state": state, "commit_id": commit})
}

#[test]
fn a_writer_s_last_review_approving_the_head_approves_it() {
    let approve = reviewed("r", "APPROVED", "h2");
    let read = |reviews: Value, role: Value| {
        approved_by(
            Forge::GitHub,
            vec![ok(&reviews), ok(&role)],
            Permission::Write,
        )
        .unwrap()
    };
    let reader_only = json!({"user": {"permissions": {"pull": true}}});
    assert_eq!(read(json!([approve.clone()]), writer()), (true, true));
    assert_eq!(read(json!([approve.clone()]), reader_only), (false, true));
    assert_eq!(
        read(json!([reviewed("r", "APPROVED", "h1")]), writer()),
        (false, true)
    );
    for after in ["DISMISSED", "CHANGES_REQUESTED", "SOMETHING_NEW"] {
        let reviews = json!([approve.clone(), reviewed("r", after, "h2")]);
        assert_eq!(read(reviews, writer()), (false, true));
    }
    let commented = json!([approve.clone(), reviewed("r", "COMMENTED", "h2")]);
    assert_eq!(read(commented, writer()), (true, true));
    let above = approved_by(
        Forge::GitHub,
        vec![ok(&json!([approve])), ok(&writer())],
        Permission::Maintain,
    )
    .unwrap();
    assert_eq!(above, (false, true));
}

#[test]
fn an_approved_queue_reads_each_entry_s_reviews_whole_or_never_stands() {
    let asked = |reviews: Vec<Response>| {
        let mut answers = vec![
            ok(&pull("h2")),
            ok(&json!([labeled("labeled", "w", 10)])),
            ok(&writer()),
            ok(&json!([])),
        ];
        answers.extend(reviews);
        entered_least(
            Forge::GitHub,
            answers,
            (&Enter::default(), Permission::Write, Vouches::Approved),
        )
        .unwrap()
    };
    let approved = asked(vec![ok(&json!([reviewed("w", "APPROVED", "h2")]))]);
    assert!(approved[0].approved);
    assert!(approved[0].whole);
    let partial = asked(pages(5, 100, |_| reviewed("x", "COMMENTED", "h2")));
    assert!(!partial[0].approved);
    assert!(!partial[0].whole);
    let unasked = entered(
        Forge::GitHub,
        vec![
            ok(&pull("h2")),
            ok(&json!([labeled("labeled", "w", 10)])),
            ok(&writer()),
            ok(&json!([])),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert!(!unasked[0].approved);
    assert!(unasked[0].whole);
}

#[test]
fn a_comment_naming_a_commit_stands_where_the_pushes_cannot_be_read() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let read = entered(
        Forge::GitHub,
        vec![
            ok(&pull(sha)),
            ok(&json!([{"user": {"login": "w"}, "body": format!("/merge {sha}"), "created_at": stamp(10), "updated_at": stamp(10)}])),
            ok(&writer()),
            refused(404),
        ],
        &Enter::Comment("/merge".to_owned()),
    )
    .unwrap();
    assert_eq!(read[0].gesture.named.as_deref(), Some(sha));
    assert_eq!(read[0].gesture.head, None);
}

#[test]
fn github_s_activity_places_a_gesture_only_after_a_push_it_lists() {
    let read = |pushes: Value| {
        entered(
            Forge::GitHub,
            vec![
                ok(&pull("h2")),
                ok(&json!([labeled("labeled", "w", 10)])),
                ok(&writer()),
                ok(&pushes),
            ],
            &Enter::default(),
        )
        .unwrap()
    };
    assert_eq!(read(json!([]))[0].gesture.head, None);
    assert_eq!(
        read(json!([pushed("h1", 5)]))[0].gesture.head.as_deref(),
        Some("h2")
    );
}

#[test]
fn an_event_on_the_label_that_neither_adds_nor_removes_it_is_no_mark() {
    let read = entered(
        Forge::GitHub,
        vec![
            ok(&pull("h2")),
            ok(&json!([
                labeled("labeled", "w", 10),
                labeled("renamed", "w", 12)
            ])),
            ok(&writer()),
            ok(&json!([pushed("h1", 5)])),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].gesture.at, at(10));
}

#[test]
fn a_pull_request_whose_head_repository_is_gone_has_no_head_on_record() {
    let mut gone = pull("h2");
    gone[0]["head"]["repo"] = Value::Null;
    let read = entered(
        Forge::GitHub,
        vec![
            ok(&gone),
            ok(&json!([labeled("labeled", "w", 10)])),
            ok(&writer()),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert_eq!(read[0].gesture.head, None);
    assert_eq!(read[0].arrived, None);
}

#[test]
fn a_push_github_stamps_within_the_skew_before_a_label_counts_as_after_it() {
    let read = |second: u32| {
        entered(
            Forge::GitHub,
            vec![
                ok(&pull("h3")),
                ok(&json!([{
                    "event": "labeled",
                    "actor": {"login": "w", "id": 2},
                    "created_at": at_second(10, second).to_string(),
                    "label": {"name": LABEL}
                }])),
                ok(&writer()),
                ok(&json!([pushed("h2", 10), pushed("h1", 5)])),
            ],
            &Enter::default(),
        )
        .unwrap()
    };
    assert_eq!(read(5)[0].gesture.head.as_deref(), Some("h2"));
    assert_eq!(read(6)[0].gesture.head.as_deref(), Some("h3"));
}
