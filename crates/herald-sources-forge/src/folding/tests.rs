use herald_core::{Described, Lifecycle, Mergeable, Turn, Verdict, Visibility};
use jiff::Timestamp;
use serde_json::{Value, json};

use super::*;

const PATH: &str = "nightworksio/herald";

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn repository() -> Value {
    json!({ "id": 42, "full_name": PATH, "html_url": "https://github.com/nightworksio/herald",
        "private": false })
}

fn pull(state: &str, merged: bool) -> Value {
    json!({
        "number": 7, "title": "Add a queue", "html_url": "https://github.com/nightworksio/herald/pull/7",
        "state": state, "merged": merged, "user": { "login": "renovate[bot]", "type": "Bot" },
        "draft": false, "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
        "head": { "sha": "abc" }, "labels": [{ "name": "deps" }],
        "requested_reviewers": [{ "login": "octo" }], "requested_teams": [{ "slug": "core" }],
        "mergeable": false
    })
}

fn delivered(forge: Forge, name: &str, payload: &Value) -> Result<Option<Folding>, SourceError> {
    folded(
        forge,
        (name, payload.to_string().as_bytes()),
        Of::Repository(PATH),
    )
}

fn said(forge: Forge, name: &str, payload: &Value) -> Vec<Folded> {
    delivered(forge, name, payload)
        .unwrap()
        .map(|folding| folding.folded)
        .unwrap_or_default()
}

fn described(folded: &[Folded]) -> &Described {
    match folded.first() {
        Some(Folded::Pull(described)) => described,
        other => panic!("no pull request described: {other:?}"),
    }
}

#[test]
fn each_forge_s_events_kept_mode_folds_are_named() {
    for (forge, name, delivery) in [
        (Forge::GitHub, "pull_request", Some(Delivery::Pull)),
        (Forge::Forgejo, "pull_request", Some(Delivery::Pull)),
        (Forge::GitLab, "Merge Request Hook", Some(Delivery::Pull)),
        (
            Forge::GitHub,
            "pull_request_review",
            Some(Delivery::Review(None)),
        ),
        (
            Forge::Forgejo,
            "pull_request_approved",
            Some(Delivery::Review(Some(true))),
        ),
        (
            Forge::Forgejo,
            "pull_request_rejected",
            Some(Delivery::Review(Some(false))),
        ),
        (
            Forge::GitHub,
            "pull_request_review_comment",
            Some(Delivery::Remark),
        ),
        (
            Forge::Forgejo,
            "pull_request_comment",
            Some(Delivery::Remark),
        ),
        (Forge::GitLab, "Note Hook", Some(Delivery::Remark)),
        (Forge::GitHub, "issue_comment", Some(Delivery::Comment)),
        (Forge::Forgejo, "issue_comment", Some(Delivery::Comment)),
        (Forge::GitHub, "check_run", Some(Delivery::Check)),
        (Forge::GitLab, "Pipeline Hook", Some(Delivery::Check)),
        (Forge::GitHub, "status", Some(Delivery::Status)),
        (Forge::Forgejo, "status", Some(Delivery::Status)),
        (Forge::GitHub, "repository", Some(Delivery::Repository)),
        (Forge::Forgejo, "repository", Some(Delivery::Repository)),
        (Forge::GitLab, "repository", None),
        (Forge::GitHub, "release", None),
        (Forge::GitLab, "status", None),
        (Forge::GitLab, "issue_comment", None),
        (Forge::Forgejo, "pull_request_review", None),
        (Forge::Forgejo, "check_run", None),
    ] {
        assert_eq!(
            Delivery::named(forge, name),
            delivery,
            "{name} on {forge:?}"
        );
    }
    assert_eq!(
        delivered(Forge::GitHub, "release", &json!({})).unwrap(),
        None
    );
}

#[test]
fn a_pull_request_event_describes_it_as_it_now_stands() {
    let payload = json!({ "action": "opened", "pull_request": pull("open", false),
        "repository": repository() });
    let folding = delivered(Forge::GitHub, "pull_request", &payload)
        .unwrap()
        .unwrap();
    assert_eq!(folding.repository.to_string(), "github.com/42");
    assert_eq!(
        described(&folding.folded),
        &Described {
            number: 7,
            project: "herald".to_owned(),
            repository: PATH.to_owned(),
            visibility: Some(Visibility::Public),
            title: "Add a queue".to_owned(),
            url: "https://github.com/nightworksio/herald/pull/7".to_owned(),
            author: Some(("renovate[bot]".to_owned(), true)),
            draft: false,
            labels: Some(vec!["deps".to_owned()]),
            reviewers: Some(vec!["octo".to_owned(), "nightworksio/core".to_owned()]),
            head: Some("abc".to_owned()),
            mergeable: Some(Mergeable::Conflict),
            queued: None,
            forge: None,
            opened: at("2026-11-18T10:00:00Z"),
            updated: at("2026-11-18T11:00:00Z"),
            lifecycle: Lifecycle::Open,
        }
    );
    for (action, state, merged, lifecycle, queued) in [
        ("closed", "closed", true, Lifecycle::Merged, None),
        ("closed", "closed", false, Lifecycle::Closed, None),
        ("enqueued", "open", false, Lifecycle::Open, Some(true)),
        ("dequeued", "open", false, Lifecycle::Open, Some(false)),
    ] {
        let payload = json!({ "action": action, "pull_request": pull(state, merged),
            "repository": repository() });
        let folded = said(Forge::GitHub, "pull_request", &payload);
        let described = described(&folded);
        assert_eq!((described.lifecycle, described.queued), (lifecycle, queued));
    }
    let mut clean = pull("open", false);
    clean["mergeable"] = json!(true);
    let payload =
        json!({ "action": "synchronize", "pull_request": clean, "repository": repository() });
    assert_eq!(
        described(&said(Forge::GitHub, "pull_request", &payload)).mergeable,
        Some(Mergeable::Clean)
    );
    let payload = json!({ "action": "opened", "repository": repository() });
    assert_eq!(
        delivered(Forge::GitHub, "pull_request", &payload).unwrap(),
        None
    );
}

fn review(state: &str, association: Option<&str>) -> Value {
    json!({ "action": "submitted", "pull_request": pull("open", false), "repository": repository(),
        "review": { "user": { "login": "octo" }, "state": state,
            "submitted_at": "2026-11-18T12:00:00Z", "author_association": association } })
}

#[test]
fn a_review_says_its_verdict_and_whether_its_reviewer_decides() {
    let folded = said(
        Forge::GitHub,
        "pull_request_review",
        &review("approved", Some("MEMBER")),
    );
    assert_eq!(
        folded[1],
        Folded::Review {
            number: 7,
            reviewer: "octo".to_owned(),
            verdict: Some(Verdict::Approved),
            decides: Some(true),
            at: at("2026-11-18T12:00:00Z"),
        }
    );
    let changes = said(
        Forge::GitHub,
        "pull_request_review",
        &review("CHANGES_REQUESTED", Some("NONE")),
    );
    assert!(matches!(
        &changes[1],
        Folded::Review {
            verdict: Some(Verdict::ChangesRequested),
            decides: Some(false),
            ..
        }
    ));
    let dismissed = said(
        Forge::GitHub,
        "pull_request_review",
        &review("dismissed", None),
    );
    assert!(matches!(
        &dismissed[1],
        Folded::Review {
            verdict: None,
            decides: None,
            ..
        }
    ));
    assert_eq!(
        said(
            Forge::GitHub,
            "pull_request_review",
            &review("commented", None)
        )
        .len(),
        1
    );
    let mut nameless = review("approved", None);
    nameless["review"]["user"] = Value::Null;
    assert_eq!(
        said(Forge::GitHub, "pull_request_review", &nameless).len(),
        1
    );
    let mut stateless = review("approved", None);
    stateless["review"]["state"] = Value::Null;
    assert_eq!(
        said(Forge::GitHub, "pull_request_review", &stateless).len(),
        1
    );
}

#[test]
fn forgejo_s_approval_and_rejection_are_its_sender_s_of_unknown_weight() {
    let approval = json!({ "action": "reviewed", "pull_request": pull("open", false),
        "repository": repository(), "sender": { "login": "octo" },
        "review": { "type": "pull_request_review_approved", "content": "" } });
    let folded = said(Forge::Forgejo, "pull_request_approved", &approval);
    assert_eq!(
        folded[1],
        Folded::Review {
            number: 7,
            reviewer: "octo".to_owned(),
            verdict: Some(Verdict::Approved),
            decides: None,
            at: at("2026-11-18T11:00:00Z"),
        }
    );
    let rejected = said(Forge::Forgejo, "pull_request_rejected", &approval);
    assert!(matches!(
        &rejected[1],
        Folded::Review {
            verdict: Some(Verdict::ChangesRequested),
            ..
        }
    ));
    let mut unsent = approval;
    unsent["sender"] = Value::Null;
    assert_eq!(
        said(Forge::Forgejo, "pull_request_approved", &unsent).len(),
        1
    );
}

#[test]
fn a_comment_on_a_pull_request_is_activity_and_one_on_an_issue_is_nothing() {
    let remark = json!({ "pull_request": pull("open", false), "repository": repository(),
        "comment": { "created_at": "2026-11-18T12:00:00Z", "updated_at": "2026-11-18T13:00:00Z" } });
    assert_eq!(
        said(Forge::GitHub, "pull_request_review_comment", &remark),
        [Folded::Activity {
            number: 7,
            at: at("2026-11-18T13:00:00Z")
        }]
    );
    let bare = json!({ "pull_request": pull("open", false), "repository": repository() });
    assert_eq!(
        said(Forge::Forgejo, "pull_request_comment", &bare),
        [Folded::Activity {
            number: 7,
            at: at("2026-11-18T11:00:00Z")
        }]
    );
    assert_eq!(
        said(
            Forge::Forgejo,
            "pull_request_comment",
            &json!({ "repository": repository() })
        ),
        []
    );
    let comment = |pull_request: Value| {
        json!({ "issue": { "number": 9, "pull_request": pull_request }, "repository": repository(),
            "comment": { "created_at": "2026-11-18T12:00:00Z" } })
    };
    assert_eq!(
        said(Forge::GitHub, "issue_comment", &comment(json!({}))),
        [Folded::Activity {
            number: 9,
            at: at("2026-11-18T12:00:00Z")
        }]
    );
    assert_eq!(
        said(Forge::GitHub, "issue_comment", &comment(Value::Null)),
        []
    );
}

fn run(status: &str, conclusion: Option<&str>, times: bool) -> Value {
    let (started, completed) = if times {
        (json!("2026-11-18T12:00:00Z"), json!("2026-11-18T12:05:00Z"))
    } else {
        (Value::Null, Value::Null)
    };
    json!({ "repository": repository(), "check_run": { "name": "test", "head_sha": "abc",
        "status": status, "conclusion": conclusion, "started_at": started,
        "completed_at": completed } })
}

fn outcome(folded: &[Folded]) -> Option<Outcome> {
    match folded {
        [Folded::Check { outcome, .. }] => Some(*outcome),
        _ => None,
    }
}

#[test]
fn a_check_run_and_a_status_say_their_outcome_on_their_head() {
    assert_eq!(
        said(
            Forge::GitHub,
            "check_run",
            &run("completed", Some("failure"), true)
        ),
        [Folded::Check {
            head: "abc".to_owned(),
            name: "test".to_owned(),
            outcome: Outcome::Failed,
            at: at("2026-11-18T12:05:00Z"),
        }]
    );
    let passed = said(
        Forge::GitHub,
        "check_run",
        &run("completed", Some("success"), true),
    );
    assert_eq!(outcome(&passed), Some(Outcome::Passed));
    let running = said(Forge::GitHub, "check_run", &run("in_progress", None, true));
    assert_eq!(outcome(&running), Some(Outcome::Pending));
    assert_eq!(
        said(Forge::GitHub, "check_run", &run("queued", None, false)),
        []
    );
    let status = |state: &str| {
        json!({ "repository": repository(), "sha": "abc", "state": state, "context": "ci",
            "updated_at": "2026-11-18T12:00:00Z" })
    };
    for (state, expected) in [
        ("pending", Outcome::Pending),
        ("success", Outcome::Passed),
        ("warning", Outcome::Passed),
        ("error", Outcome::Failed),
        ("failure", Outcome::Failed),
    ] {
        assert_eq!(
            outcome(&said(Forge::Forgejo, "status", &status(state))),
            Some(expected)
        );
    }
    let mut unnamed = status("success");
    unnamed["context"] = Value::Null;
    assert_eq!(said(Forge::GitHub, "status", &unnamed), []);
}

#[test]
fn a_delivery_of_another_repository_or_unreadable_is_refused() {
    let mut other = repository();
    other["full_name"] = json!("someone/else");
    let payload = json!({ "pull_request": pull("open", false), "repository": other });
    assert!(delivered(Forge::GitHub, "pull_request", &payload).is_err());
    let mut pageless = repository();
    pageless["html_url"] = json!("not a page");
    let payload = json!({ "pull_request": pull("open", false), "repository": pageless });
    assert!(delivered(Forge::GitHub, "pull_request", &payload).is_err());
    assert!(delivered(Forge::GitHub, "pull_request", &json!([])).is_err());
    assert!(delivered(Forge::GitLab, "Merge Request Hook", &json!([])).is_err());
}

fn project() -> Value {
    json!({ "id": 9, "path_with_namespace": PATH, "web_url": "https://gitlab.com/nightworksio/herald",
        "visibility_level": 20 })
}

fn merge(action: &str, state: &str, status: Option<&str>) -> Value {
    json!({
        "user": { "username": "project_9_bot_abc" }, "project": project(),
        "object_attributes": { "iid": 3, "title": "Bump", "url": "https://gitlab.com/nightworksio/herald/-/merge_requests/3",
            "state": state, "action": action, "draft": true, "last_commit": { "id": "def" },
            "detailed_merge_status": status, "created_at": "2026-11-18 10:00:00 UTC",
            "updated_at": "2026-11-18 11:00:00 UTC" },
        "labels": [{ "title": "deps" }], "reviewers": [{ "username": "octo" }]
    })
}

#[test]
fn a_merge_request_event_describes_it_and_an_approval_is_its_actor_s() {
    let folding = delivered(
        Forge::GitLab,
        "Merge Request Hook",
        &merge("open", "opened", Some("conflict")),
    )
    .unwrap()
    .unwrap();
    assert_eq!(folding.repository.to_string(), "gitlab.com/9");
    let opened = described(&folding.folded);
    assert_eq!(opened.author, Some(("project_9_bot_abc".to_owned(), true)));
    assert_eq!(
        (opened.number, opened.draft, opened.head.as_deref()),
        (3, true, Some("def"))
    );
    assert_eq!(opened.labels, Some(vec!["deps".to_owned()]));
    assert_eq!(opened.reviewers, Some(vec!["octo".to_owned()]));
    assert_eq!(opened.mergeable, Some(Mergeable::Conflict));
    assert_eq!(
        opened.forge,
        Some((Turn::WaitingOnAuthor, "conflict".to_owned()))
    );
    assert_eq!(opened.visibility, Some(Visibility::Public));
    assert_eq!(opened.lifecycle, Lifecycle::Open);
    assert_eq!(opened.updated, at("2026-11-18T11:00:00Z"));
    let approved = said(
        Forge::GitLab,
        "Merge Request Hook",
        &merge("approved", "opened", Some("mergeable")),
    );
    let updated = described(&approved);
    assert_eq!(
        (updated.author.clone(), updated.mergeable),
        (None, Some(Mergeable::Clean))
    );
    assert_eq!(
        approved[1],
        Folded::Review {
            number: 3,
            reviewer: "project_9_bot_abc".to_owned(),
            verdict: Some(Verdict::Approved),
            decides: Some(true),
            at: at("2026-11-18T11:00:00Z"),
        }
    );
    let withdrawn = said(
        Forge::GitLab,
        "Merge Request Hook",
        &merge("unapproval", "opened", None),
    );
    assert!(matches!(
        &withdrawn[1],
        Folded::Review { verdict: None, .. }
    ));
    assert_eq!(described(&withdrawn).mergeable, None);
    for (state, lifecycle) in [("merged", Lifecycle::Merged), ("closed", Lifecycle::Closed)] {
        let folded = said(
            Forge::GitLab,
            "Merge Request Hook",
            &merge("update", state, None),
        );
        assert_eq!((folded.len(), described(&folded).lifecycle), (1, lifecycle));
    }
    let mut timeless = merge("update", "opened", None);
    timeless["object_attributes"]["created_at"] = Value::Null;
    timeless["object_attributes"]["updated_at"] = Value::Null;
    assert_eq!(
        delivered(Forge::GitLab, "Merge Request Hook", &timeless).unwrap(),
        None
    );
}

#[test]
fn a_pipeline_is_its_head_s_one_check_and_a_note_on_a_merge_request_is_activity() {
    let pipeline = |status: &str| {
        json!({ "project": project(), "object_attributes": { "sha": "def", "status": status,
            "created_at": "2026-11-18 10:00:00 UTC", "finished_at": "2026-11-18 10:30:00 UTC" } })
    };
    assert_eq!(
        said(Forge::GitLab, "Pipeline Hook", &pipeline("failed")),
        [Folded::Check {
            head: "def".to_owned(),
            name: crate::queue::PIPELINE.to_owned(),
            outcome: Outcome::Failed,
            at: at("2026-11-18T10:30:00Z"),
        }]
    );
    for (status, expected) in [
        ("success", Outcome::Passed),
        ("manual", Outcome::Passed),
        ("canceled", Outcome::Failed),
        ("running", Outcome::Pending),
    ] {
        assert_eq!(
            outcome(&said(Forge::GitLab, "Pipeline Hook", &pipeline(status))),
            Some(expected)
        );
    }
    let mut shaless = pipeline("success");
    shaless["object_attributes"]["sha"] = Value::Null;
    assert_eq!(said(Forge::GitLab, "Pipeline Hook", &shaless), []);
    let note = |kind: &str| {
        json!({ "project": project(), "merge_request": { "iid": 3 },
            "object_attributes": { "noteable_type": kind, "created_at": "2026-11-18 12:00:00 UTC" } })
    };
    assert_eq!(
        said(Forge::GitLab, "Note Hook", &note("MergeRequest")),
        [Folded::Activity {
            number: 3,
            at: at("2026-11-18T12:00:00Z")
        }]
    );
    assert_eq!(said(Forge::GitLab, "Note Hook", &note("Issue")), []);
}

#[test]
fn a_repository_delivery_says_who_may_now_see_every_pull_request_of_it() {
    let mut private = repository();
    private["private"] = json!(true);
    let payload = json!({ "action": "privatized", "repository": private });
    assert_eq!(
        said(Forge::GitHub, "repository", &payload),
        [Folded::Visibility(Visibility::Private)]
    );
    let mut unsaid = repository();
    unsaid["private"] = Value::Null;
    let payload = json!({ "action": "renamed", "repository": unsaid });
    assert_eq!(said(Forge::Forgejo, "repository", &payload), []);
}

#[test]
fn an_owner_s_delivery_is_folded_where_the_owner_holds_its_repository_and_named_by_it() {
    let payload = json!({ "action": "opened", "pull_request": pull("open", false),
        "repository": repository() });
    let bytes = payload.to_string();
    let owned = folded(
        Forge::GitHub,
        ("pull_request", bytes.as_bytes()),
        Of::Owner("NightworksIO", false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(described(&owned.folded).project, "herald");
    let unheld = folded(
        Forge::GitHub,
        ("pull_request", bytes.as_bytes()),
        Of::Owner("lemonfiber", false),
    )
    .unwrap_err();
    assert!(
        unheld
            .to_string()
            .contains("not of a repository lemonfiber, what its source names, holds"),
        "{unheld}"
    );
}
