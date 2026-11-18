use serde_json::{Value, json};

use super::*;

fn request(fields: &Value) -> Value {
    let mut request = json!({
        "iid": 3, "title": "Add feature", "web_url": "https://gitlab.com/g/p/-/merge_requests/3",
        "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z"
    });
    for (key, value) in fields.as_object().unwrap() {
        request[key] = value.clone();
    }
    request
}

fn one(fields: &Value, known: Option<Visibility>) -> Pull {
    let body = Value::from(vec![request(fields)]).to_string();
    page(body.as_bytes(), 1 << 20, ("p", "g/p", known))
        .unwrap()
        .pulls
        .remove(0)
}

fn status(status: &str, reviewers: bool) -> Pull {
    let reviewers = if reviewers {
        json!([{ "username": "ann" }])
    } else {
        json!([])
    };
    one(
        &json!({ "detailed_merge_status": status, "reviewers": reviewers }),
        None,
    )
}

#[test]
fn a_bare_merge_request_reads_as_a_private_unasked_one_of_its_project() {
    let pull = one(&json!({}), None);
    assert_eq!(
        (pull.project.as_str(), pull.repository.as_str()),
        ("p", "g/p")
    );
    assert_eq!(pull.visibility, Visibility::Private);
    assert_eq!(pull.number, 3);
    assert_eq!(pull.title, "Add feature");
    assert_eq!(pull.url, "https://gitlab.com/g/p/-/merge_requests/3");
    assert_eq!(pull.author, None);
    assert!(!pull.bot && !pull.draft && !pull.queued);
    assert_eq!(pull.opened.to_string(), "2026-11-18T10:00:00Z");
    assert_eq!(pull.updated.to_string(), "2026-11-18T11:00:00Z");
    assert_eq!(
        (pull.review, pull.checks, pull.mergeable, pull.forge),
        (Review::Unasked, Checks::Absent, Mergeable::Unknown, None)
    );
    assert_eq!(
        one(&json!({}), Some(Visibility::Public)).visibility,
        Visibility::Public
    );
}

#[test]
fn authors_labels_drafts_and_bots_are_read() {
    let pull = one(
        &json!({ "draft": true, "author": { "username": "ann" }, "labels": ["a", "b"] }),
        None,
    );
    assert_eq!(pull.author.as_deref(), Some("ann"));
    assert!(pull.draft && !pull.bot);
    assert_eq!(pull.labels, ["a", "b"]);
    assert!(
        one(
            &json!({ "author": { "username": "project_7_bot_abc" } }),
            None
        )
        .bot
    );
    assert!(one(&json!({ "author": { "username": "renovate-bot" } }), None).bot);
    let headed = one(&json!({ "sha": "abc" }), None);
    assert_eq!(
        (headed.head.as_deref(), headed.unread),
        (Some("abc"), false)
    );
}

#[test]
fn the_detailed_merge_status_tells_reviews_checks_and_mergeability() {
    let mergeable = status("mergeable", false);
    assert_eq!(
        (mergeable.review, mergeable.checks, mergeable.mergeable),
        (Review::Approved, Checks::Passed, Mergeable::Clean)
    );
    let changes = status("requested_changes", true);
    assert_eq!(
        (changes.review, changes.checks),
        (Review::ChangesRequested, Checks::Absent)
    );
    let running = status("ci_still_running", true);
    assert_eq!(
        (running.review, running.checks),
        (Review::Requested, Checks::Pending)
    );
    let red = status("ci_must_pass", false);
    assert_eq!(
        (red.review, red.checks),
        (Review::Unasked, Checks::Failed(vec!["pipeline".to_owned()]))
    );
    let unapproved = status("not_approved", true);
    assert_eq!(
        (unapproved.review, unapproved.checks, unapproved.mergeable),
        (Review::Requested, Checks::Absent, Mergeable::Unknown)
    );
    assert_eq!(status("conflict", false).mergeable, Mergeable::Conflict);
    assert_eq!(
        one(
            &json!({ "has_conflicts": true, "detailed_merge_status": "mergeable" }),
            None
        )
        .mergeable,
        Mergeable::Conflict
    );
}

#[test]
fn the_forge_s_own_turn_is_its_detailed_merge_status_mapped() {
    let turns = [
        ("mergeable", Turn::WaitingOnMerge),
        ("draft_status", Turn::Draft),
        ("ci_still_running", Turn::WaitingOnChecks),
        ("checking", Turn::WaitingOnChecks),
        ("unchecked", Turn::WaitingOnChecks),
        ("preparing", Turn::WaitingOnChecks),
        ("approvals_syncing", Turn::WaitingOnChecks),
        ("ci_must_pass", Turn::WaitingOnAuthor),
        ("conflict", Turn::WaitingOnAuthor),
        ("need_rebase", Turn::WaitingOnAuthor),
        ("requested_changes", Turn::WaitingOnAuthor),
        ("discussions_not_resolved", Turn::WaitingOnAuthor),
        ("not_approved", Turn::WaitingOnReview),
        ("blocked_status", Turn::Unknown),
    ];
    for (named, turn) in turns {
        assert_eq!(status(named, false).forge, Some((turn, named.to_owned())));
    }
}

#[test]
fn a_page_is_asked_by_its_number_and_reads_within_its_size() {
    assert_eq!(
        path(2),
        "/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=100&page=2"
    );
    assert!(matches!(
        page(b"[]", 1, ("p", "g/p", None)),
        Err(SourceError::TooLarge {
            what: WHAT,
            limit: 1
        })
    ));
    assert_eq!(page(b"[]", 2, ("p", "g/p", None)).unwrap(), Page::default());
}
