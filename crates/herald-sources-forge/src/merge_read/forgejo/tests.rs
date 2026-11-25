use herald_core::Permission;
use herald_core::merging::{Check, CheckState, Enter};
use serde_json::{Value, json};

use super::super::tests::{
    LABEL, approved_by, at, base, branched, entered, entry, ok, pages, proved, refused, stamp,
};
use super::*;
use crate::Forge;

fn pulls(head: &str) -> Value {
    json!([
        {"number": 3, "html_url": "u3", "head": {"sha": "other"}, "base": {"ref": "dev"}, "labels": [{"name": LABEL}]},
        {"number": 4, "html_url": "u4", "draft": false, "head": {"sha": head, "ref": "f", "repo": {"full_name": "o/r"}},
         "base": {"ref": "main"}, "user": {"login": "author"}, "labels": [{"name": LABEL}]}
    ])
}

fn labelled(body: &str, login: &str, minute: u32) -> Value {
    json!({"type": "label", "user": {"login": login}, "created_at": stamp(minute), "body": body, "label": {"name": LABEL}})
}

fn push(force: bool, ids: &[&str], minute: u32) -> Value {
    let body = json!({"is_force_push": force, "commit_ids": ids}).to_string();
    json!({"type": "pull_push", "user": {"login": "author"}, "created_at": stamp(minute), "body": body})
}

#[test]
fn a_label_a_writer_added_stands_where_no_push_came_after_it() {
    let timeline = json!([push(false, &["h1"], 5), labelled("1", "w", 10)]);
    let read = entered(
        Forge::Forgejo,
        vec![
            ok(&pulls("h1")),
            ok(&json!([])),
            ok(&timeline),
            ok(&json!([])),
            ok(&json!({"permission": "write"})),
            ok(&timeline),
            ok(&json!([])),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].number, 4);
    assert_eq!(read[0].gesture.head.as_deref(), Some("h1"));
    assert_eq!(read[0].gesture.role, Some(Permission::Write));
    assert_eq!(read[0].arrived, Some(at(5)));
    assert!(read[0].whole);
}

#[test]
fn a_push_after_the_label_vouches_only_for_the_head_a_forced_one_moved_from() {
    let forced = vec![push(true, &["h1", "h2"], 20)];
    let plain = vec![push(false, &["h2"], 20)];
    let entries =
        |items: Vec<Value>| -> Vec<Entry> { serde_json::from_value(Value::Array(items)).unwrap() };
    let forced = timeline_pushes(entries(forced));
    assert_eq!(forced[0].before.as_deref(), Some("h1"));
    let plain = timeline_pushes(entries(plain));
    assert_eq!(plain[0].before, None);
}

#[test]
fn a_label_removed_past_the_timeline_herald_reads_is_never_read_as_standing() {
    let mut answers = vec![ok(&pulls("h1")), ok(&json!([]))];
    let item = |index: usize| {
        if index == 0 {
            labelled("1", "w", 10)
        } else {
            json!({"type": "comment", "user": {"login": "c"}, "created_at": stamp(11), "body": "hi"})
        }
    };
    answers.extend(pages(10, 50, item));
    answers.push(ok(&json!({"permission": "admin"})));
    answers.extend(pages(10, 50, item));
    let read = entered(Forge::Forgejo, answers, &Enter::default()).unwrap();
    assert_eq!(read.len(), 1);
    assert!(!read[0].whole);
}

#[test]
fn a_role_forgejo_refuses_to_say_stops_the_read() {
    let error = entered(
        Forge::Forgejo,
        vec![
            ok(&pulls("h1")),
            ok(&json!([])),
            ok(&json!([labelled("1", "w", 10)])),
            ok(&json!([])),
            refused(403),
        ],
        &Enter::default(),
    )
    .unwrap_err();
    assert!(error.into_reason().contains("administers the repository"));
}

#[test]
fn a_pull_request_is_proved_from_its_merge_base_and_head_statuses() {
    let answers = |total: usize| {
        vec![
            ok(&json!({"merge_base": "b1", "mergeable": true})),
            ok(&json!({"total_count": total, "statuses": [
                {"context": "ci/test", "status": "success"},
                {"context": "ci/lint", "status": "pending"},
                {"context": "deploy", "status": "warning"}
            ]})),
        ]
    };
    let read = proved(
        Forge::Forgejo,
        answers(3),
        (&entry("h1"), &base(&["ci/*", "docs"], true)),
    )
    .unwrap();
    assert!(read.up_to_date && read.whole);
    assert_eq!((read.mergeable, read.clean), (Some(true), Some(true)));
    let mut conflicting = answers(3);
    conflicting[0] = ok(&json!({"merge_base": "b1", "mergeable": false}));
    let read_conflicting = proved(
        Forge::Forgejo,
        conflicting,
        (&entry("h1"), &base(&[], true)),
    )
    .unwrap();
    assert_eq!(read_conflicting.clean, Some(false));
    assert_eq!(read.required, ["ci/lint", "ci/test", "docs"]);
    let named = |name: &str, state| Check {
        name: name.to_owned(),
        state,
    };
    assert_eq!(
        read.checks,
        [
            named("ci/test", CheckState::Passed),
            named("ci/lint", CheckState::Pending),
            named("deploy", CheckState::Failed)
        ]
    );
    let partial = proved(Forge::Forgejo, answers(4), (&entry("h1"), &base(&[], true))).unwrap();
    assert!(!partial.whole);
    let unglobbed = proved(
        Forge::Forgejo,
        answers(3),
        (&entry("h1"), &base(&["ci/["], true)),
    )
    .unwrap();
    assert_eq!(unglobbed.required, ["ci/["]);
}

#[test]
fn a_branch_s_protection_names_its_required_status_checks() {
    let answers = |enabled: bool| {
        vec![
            ok(&json!({"private": false, "internal": false})),
            ok(
                &json!({"commit": {"id": "b1"}, "enable_status_check": enabled, "status_check_contexts": ["ci/*"]}),
            ),
        ]
    };
    let read = branched(Forge::Forgejo, answers(true)).unwrap();
    assert_eq!(read.required, ["ci/*"]);
    assert_eq!(read.queue, Some(false));
    assert_eq!(read.visibility, herald_core::Visibility::Public);
    let bare = branched(Forge::Forgejo, answers(false)).unwrap();
    assert_eq!(bare.required, Vec::<String>::new());
}

#[test]
fn a_writer_s_approval_of_the_head_approves_it_unless_dismissed() {
    let reviewed = |state: &str, dismissed: bool| json!({"user": {"login": "r"}, "state": state, "commit_id": "h2", "dismissed": dismissed});
    let read = |reviews: Value| {
        approved_by(
            Forge::Forgejo,
            vec![
                ok(&reviews),
                ok(&json!([])),
                ok(&json!({"permission": "write"})),
            ],
            Permission::Write,
        )
        .unwrap()
    };
    assert_eq!(read(json!([reviewed("APPROVED", false)])), (true, true));
    assert_eq!(read(json!([reviewed("APPROVED", true)])), (false, true));
    let later = json!([
        reviewed("APPROVED", false),
        reviewed("REQUEST_CHANGES", false)
    ]);
    assert_eq!(read(later), (false, true));
    let neutral = json!([
        reviewed("APPROVED", false),
        reviewed("REQUEST_REVIEW", false)
    ]);
    assert_eq!(read(neutral), (true, true));
}

#[test]
fn a_command_comment_by_a_writer_queues_and_an_edited_or_nameless_one_does_not() {
    let comment = |login: Value, minute: u32, updated: u32| json!({"type": "comment", "user": login, "created_at": stamp(minute), "updated_at": stamp(updated), "body": "/merge"});
    let timeline = json!([
        push(false, &["h1"], 5),
        comment(json!({"login": "w"}), 10, 10),
        comment(Value::Null, 11, 11),
        comment(json!({"login": "x"}), 12, 13)
    ]);
    let read = entered(
        Forge::Forgejo,
        vec![
            ok(&pulls("h1")),
            ok(&json!([])),
            ok(&timeline),
            ok(&json!([])),
            ok(&json!({"permission": "write"})),
            ok(&timeline),
            ok(&json!([])),
        ],
        &Enter::Comment("/merge".to_owned()),
    )
    .unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].gesture.by, "w");
    assert_eq!(read[0].gesture.at, at(10));
    assert_eq!(read[0].gesture.head.as_deref(), Some("h1"));
}

#[test]
fn a_timeline_cut_before_its_newest_entries_places_no_head() {
    let item = |index: usize| match index {
        0 => push(true, &["h0", "h1"], 5),
        1 => labelled("1", "w", 10),
        _ => {
            json!({"type": "comment", "user": {"login": "c"}, "created_at": stamp(11), "body": "hi"})
        }
    };
    let mut answers = vec![ok(&pulls("h9")), ok(&json!([]))];
    answers.extend(pages(10, 50, item));
    answers.push(ok(&json!({"permission": "write"})));
    answers.extend(pages(10, 50, item));
    let read = entered(Forge::Forgejo, answers, &Enter::default()).unwrap();
    assert_eq!(read[0].gesture.head, None);
    assert_eq!(read[0].arrived, None);
    assert!(!read[0].whole);
}
