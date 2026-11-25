use herald_core::Permission;
use herald_core::merging::{CheckState, Enter};
use serde_json::{Value, json};

use super::super::tests::{
    LABEL, approved_by, approved_within, at, at_second, base, branched, entered, entered_least,
    entry, ok, pages, proved, refused, stamp,
};
use super::*;
use crate::Forge;

fn merge(head: &str) -> Value {
    json!([{
        "iid": 7,
        "web_url": "https://gitlab.com/o/r/-/merge_requests/7",
        "draft": false,
        "sha": head,
        "author": {"id": 1, "username": "author"},
        "labels": [LABEL],
        "source_branch": "feature"
    }])
}

fn label_event(action: &str, id: u64, minute: u32) -> Value {
    json!({"user": {"id": id, "username": format!("u{id}")}, "created_at": stamp(minute), "label": {"name": LABEL}, "action": action})
}

fn version(head: &str, minute: u32) -> Value {
    json!({"head_commit_sha": head, "created_at": stamp(minute)})
}

#[test]
fn a_label_a_developer_added_stands_on_the_version_gitlab_records_then() {
    let read = entered(
        Forge::GitLab,
        vec![
            ok(&merge("h3")),
            ok(&json!([label_event("add", 2, 10)])),
            ok(&json!({"access_level": 30})),
            ok(&json!([
                version("h3", 20),
                version("h1", 1),
                version("h2", 5)
            ])),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].gesture.by, "u2");
    assert_eq!(read[0].gesture.role, Some(Permission::Write));
    assert_eq!(read[0].gesture.head.as_deref(), Some("h2"));
    assert_eq!(read[0].arrived, Some(at(20)));
    assert!(read[0].whole);
}

#[test]
fn a_label_by_a_reporter_or_a_non_member_queues_nothing() {
    for answer in [ok(&json!({"access_level": 20})), refused(404)] {
        let read = entered(
            Forge::GitLab,
            vec![
                ok(&merge("h1")),
                ok(&json!([label_event("add", 2, 10)])),
                answer,
            ],
            &Enter::default(),
        )
        .unwrap();
        assert_eq!(read, []);
    }
}

#[test]
fn a_label_removed_past_the_events_herald_reads_is_never_read_as_standing() {
    let mut answers = vec![ok(&merge("h1"))];
    answers.extend(pages(5, 100, |index| {
        if index == 0 {
            label_event("add", 2, 10)
        } else {
            json!({"user": null, "created_at": stamp(11), "label": {"name": "other"}, "action": "add"})
        }
    }));
    answers.push(ok(&json!({"access_level": 40})));
    answers.push(ok(&json!([version("h1", 1)])));
    let read = entered(Forge::GitLab, answers, &Enter::default()).unwrap();
    assert_eq!(read.len(), 1);
    assert!(!read[0].whole);
}

#[test]
fn gitlab_s_own_notes_are_no_comment() {
    let note = |body: &str, system: bool| json!({"author": {"id": 2, "username": "w"}, "body": body, "created_at": stamp(10), "updated_at": stamp(10), "system": system});
    let read = entered(
        Forge::GitLab,
        vec![ok(&merge("h1")), ok(&json!([note("/merge", true)]))],
        &Enter::Comment("/merge".to_owned()),
    )
    .unwrap();
    assert_eq!(read, []);
}

#[test]
fn a_merge_request_is_proved_from_its_divergence_state_and_head_pipeline() {
    let answer = |diverged: u64, sha: &str, status: &str| {
        vec![ok(&json!({
            "detailed_merge_status": "mergeable",
            "diverged_commits_count": diverged,
            "head_pipeline": {"sha": sha, "status": status}
        }))]
    };
    let read = proved(
        Forge::GitLab,
        answer(0, "h1", "success"),
        (&entry("h1"), &base(&["pipeline"], true)),
    )
    .unwrap();
    assert!(read.up_to_date && read.whole);
    assert_eq!(read.mergeable, Some(true));
    assert_eq!(read.checks.len(), 1);
    assert_eq!(read.checks[0].state, CheckState::Passed);
    let stale = proved(
        Forge::GitLab,
        answer(2, "h0", "failed"),
        (&entry("h1"), &base(&[], true)),
    )
    .unwrap();
    assert!(!stale.up_to_date);
    assert_eq!(stale.checks, []);
    assert_eq!(read.clean, None);
    for conflicts in [true, false] {
        let answer = vec![ok(&json!({
            "detailed_merge_status": "mergeable",
            "diverged_commits_count": 2,
            "has_conflicts": conflicts
        }))];
        let read = proved(Forge::GitLab, answer, (&entry("h1"), &base(&[], true))).unwrap();
        assert_eq!(read.clean, Some(!conflicts));
    }
}

#[test]
fn pipelines_and_merge_states_map_to_herald_s() {
    use CheckState::{Failed, Passed, Pending};
    let piped: Vec<CheckState> = [
        "success", "skipped", "failed", "canceled", "running", "manual",
    ]
    .into_iter()
    .map(piped)
    .collect();
    assert_eq!(piped, [Passed, Passed, Failed, Failed, Pending, Pending]);
    assert_eq!(mergeable(Some("mergeable")), Some(true));
    for state in UNSETTLED {
        assert_eq!(mergeable(Some(state)), None, "{state}");
    }
    for state in [
        "conflict",
        "not_approved",
        "draft_status",
        "discussions_not_resolved",
    ] {
        assert_eq!(mergeable(Some(state)), Some(false), "{state}");
    }
    assert_eq!(mergeable(None), None);
}

#[test]
fn a_project_requiring_its_pipeline_requires_the_pipeline_check() {
    let read = branched(
        Forge::GitLab,
        vec![
            ok(&json!({"visibility": "private", "merge_trains_enabled": true, "only_allow_merge_if_pipeline_succeeds": true})),
            ok(&json!({"commit": {"id": "b1"}})),
        ],
    )
    .unwrap();
    assert_eq!(read.required, ["pipeline"]);
    assert_eq!(read.queue, Some(true));
    assert_eq!(read.head, "b1");
    assert_eq!(read.visibility, herald_core::Visibility::Private);
    let bare = branched(
        Forge::GitLab,
        vec![
            ok(&json!({"visibility": "public"})),
            ok(&json!({"commit": {"id": "b1"}})),
        ],
    )
    .unwrap();
    assert_eq!(bare.required, Vec::<String>::new());
    assert_eq!(bare.queue, None);
}

#[test]
fn an_approval_counts_only_where_gitlab_noted_it_after_the_latest_push() {
    let note = |body: &str, minute: u32, system: bool| json!({"author": {"username": "r", "id": 7}, "body": body, "created_at": stamp(minute), "updated_at": stamp(minute), "system": system});
    let read = |notes: Value| {
        approved_by(
            Forge::GitLab,
            vec![
                ok(&json!([
                    {"head_commit_sha": "h1", "created_at": stamp(5)},
                    {"head_commit_sha": "h2", "created_at": stamp(20)}
                ])),
                ok(&notes),
                ok(&json!({"access_level": 30})),
            ],
            Permission::Write,
        )
        .unwrap()
    };
    let approved = "approved this merge request";
    assert_eq!(read(json!([note(approved, 30, true)])), (true, true));
    assert_eq!(read(json!([note(approved, 10, true)])), (false, true));
    assert_eq!(read(json!([note(approved, 20, true)])), (false, true));
    assert_eq!(read(json!([note(approved, 30, false)])), (false, true));
    let taken_back = json!([
        note(approved, 30, true),
        note("unapproved this merge request", 31, true)
    ]);
    assert_eq!(read(taken_back), (false, true));
    let other = json!([
        note(approved, 30, true),
        note("requested review from @r", 31, true)
    ]);
    assert_eq!(read(other), (true, true));
    let refused_versions = approved_by(Forge::GitLab, vec![refused(404)], Permission::Write);
    assert!(refused_versions.is_err());
}

#[test]
fn an_approval_noted_within_the_skew_after_the_latest_push_proves_nothing() {
    let read = |second: u32| {
        let note = json!([{"author": {"username": "r", "id": 7}, "body": "approved this merge request", "created_at": at_second(20, second).to_string(), "updated_at": at_second(20, second).to_string(), "system": true}]);
        approved_within(
            Forge::GitLab,
            vec![
                ok(&json!([
                    {"head_commit_sha": "h1", "created_at": stamp(5)},
                    {"head_commit_sha": "h2", "created_at": stamp(20)}
                ])),
                ok(&note),
                ok(&json!({"access_level": 30})),
            ],
            (Permission::Write, jiff::SignedDuration::from_secs(5)),
        )
        .unwrap()
    };
    assert_eq!(read(3), (false, true));
    assert_eq!(read(5), (false, true));
    assert_eq!(read(6), (true, true));
}

#[test]
fn a_label_removed_by_a_deleted_account_takes_it_out_and_other_actions_are_no_mark() {
    let read = |events: Value, answers: Vec<herald_ports::Response>| {
        let mut all = vec![
            ok(&merge("h1")),
            ok(&events),
            ok(&json!({"access_level": 30})),
        ];
        all.extend(answers);
        entered(Forge::GitLab, all, &Enter::default()).unwrap()
    };
    let removed = json!([
        label_event("add", 2, 10),
        {"user": null, "created_at": stamp(12), "label": {"name": LABEL}, "action": "remove"}
    ]);
    assert_eq!(read(removed, Vec::new()), []);
    let other = json!([label_event("add", 2, 10), label_event("move", 2, 12)]);
    let stands = read(other, vec![ok(&json!([version("h1", 1)]))]);
    assert_eq!(stands.len(), 1);
    assert_eq!(stands[0].gesture.at, at(10));
}

#[test]
fn a_command_comment_by_a_developer_queues_and_an_edited_one_does_not() {
    let note = |id: u64, minute: u32, updated: u32| json!({"author": {"id": id, "username": format!("u{id}")}, "body": "/merge", "created_at": stamp(minute), "updated_at": stamp(updated), "system": false});
    let read = entered(
        Forge::GitLab,
        vec![
            ok(&merge("h1")),
            ok(&json!([note(3, 12, 13), note(2, 10, 10), {"author": null, "body": "/merge", "created_at": stamp(11), "updated_at": stamp(11), "system": false}])),
            ok(&json!({"access_level": 30})),
            ok(&json!([version("h1", 1)])),
        ],
        &Enter::Comment("/merge".to_owned()),
    )
    .unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].gesture.by, "u2");
    assert_eq!(read[0].gesture.at, at(10));
}

#[test]
fn one_gitlab_names_no_id_for_holds_no_role() {
    let http = herald_testing::ScriptedHttp::new(Vec::new());
    let nameless = Actor {
        login: String::new(),
        id: None,
    };
    let role = pollster::block_on(role(
        &super::super::tests::reader(Forge::GitLab),
        &Erased(&http),
        &nameless,
    ));
    assert_eq!(role.unwrap(), herald_kinds::Holds::Unsaid);
}

#[test]
fn an_approved_queue_binds_an_approval_noted_within_the_skew_to_no_head() {
    let read = |second: u32| {
        let note = json!([{"author": {"username": "r", "id": 7}, "body": "approved this merge request", "created_at": at_second(20, second).to_string(), "updated_at": at_second(20, second).to_string(), "system": true}]);
        let versions = json!([version("h1", 5), version("h2", 20)]);
        entered_least(
            Forge::GitLab,
            vec![
                ok(&merge("h2")),
                ok(&json!([label_event("add", 2, 30)])),
                ok(&json!({"access_level": 30})),
                ok(&versions),
                ok(&versions),
                ok(&note),
                ok(&json!({"access_level": 30})),
            ],
            (
                &Enter::default(),
                Permission::Write,
                herald_core::merging::Vouches::Approved,
            ),
        )
        .unwrap()
    };
    assert!(!read(3)[0].approved);
    assert!(read(6)[0].approved);
}

#[test]
fn diff_versions_are_every_push_only_below_the_page_herald_reads() {
    let versions = |count: usize| -> Vec<Version> {
        (0..count)
            .map(|index| {
                serde_json::from_value(
                    json!({"head_commit_sha": format!("h{index}"), "created_at": stamp(1)}),
                )
                .unwrap()
            })
            .collect()
    };
    assert!(versioned(versions(1)).1);
    assert!(versioned(versions(MOST_VERSIONS - 1)).1);
    assert!(!versioned(versions(MOST_VERSIONS)).1);
    assert_eq!(versioned(versions(MOST_VERSIONS)).0, []);
}

#[test]
fn a_merge_request_with_more_versions_than_herald_reads_places_no_head_and_binds_no_approval() {
    let versions = |count: usize| {
        Value::Array(
            (0..count)
                .map(|index| version(&format!("h{index}"), 1))
                .collect(),
        )
    };
    let cut = entered(
        Forge::GitLab,
        vec![
            ok(&merge("h1")),
            ok(&json!([label_event("add", 2, 10)])),
            ok(&json!({"access_level": 30})),
            ok(&versions(MOST_VERSIONS)),
        ],
        &Enter::default(),
    )
    .unwrap();
    assert_eq!(cut[0].gesture.head, None);
    let note = json!([{"author": {"username": "r", "id": 7}, "body": "approved this merge request", "created_at": stamp(30), "updated_at": stamp(30), "system": true}]);
    let mut listed: Vec<Value> = (0..MOST_VERSIONS - 1)
        .map(|index| version(&format!("h{index}"), 1))
        .collect();
    listed.push(version("h2", 20));
    let approved = |listed: Vec<Value>| {
        approved_by(
            Forge::GitLab,
            vec![
                ok(&Value::Array(listed)),
                ok(&note),
                ok(&json!({"access_level": 30})),
            ],
            Permission::Write,
        )
        .unwrap()
    };
    assert_eq!(approved(listed[1..].to_vec()), (true, true));
    assert_eq!(approved(listed), (false, true));
}
