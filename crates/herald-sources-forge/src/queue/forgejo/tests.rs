use serde_json::{Value, json};

use super::*;

fn one(fields: &Value, known: Option<Visibility>) -> Pull {
    let mut listed = json!({
        "number": 5, "title": "Fix", "html_url": "https://codeberg.org/o/r/pulls/5",
        "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z"
    });
    for (key, value) in fields.as_object().unwrap() {
        listed[key] = value.clone();
    }
    let body = Value::from(vec![listed]).to_string();
    page(body.as_bytes(), 1 << 20, ("r", "o/r", known))
        .unwrap()
        .pulls
        .remove(0)
}

#[test]
fn a_bare_pull_reads_as_a_private_unasked_one_of_its_repository() {
    let pull = one(&json!({}), None);
    assert_eq!(
        (pull.project.as_str(), pull.repository.as_str()),
        ("r", "o/r")
    );
    assert_eq!(pull.visibility, Visibility::Private);
    assert_eq!((pull.number, pull.title.as_str()), (5, "Fix"));
    assert_eq!(pull.url, "https://codeberg.org/o/r/pulls/5");
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
fn authors_labels_reviewers_drafts_and_mergeability_are_read() {
    let pull = one(
        &json!({
            "draft": true, "user": { "login": "ann" }, "labels": [ { "name": "a" } ],
            "requested_reviewers": [ { "login": "bob" } ], "mergeable": true
        }),
        None,
    );
    assert_eq!(pull.author.as_deref(), Some("ann"));
    assert!(pull.draft && !pull.bot);
    assert_eq!(pull.labels, ["a"]);
    assert_eq!(
        (pull.review, pull.mergeable),
        (Review::Requested, Mergeable::Clean)
    );
    assert_eq!(
        one(&json!({ "mergeable": false }), None).mergeable,
        Mergeable::Conflict
    );
    assert!(one(&json!({ "user": { "login": "renovate[bot]" } }), None).bot);
}

#[test]
fn a_page_is_asked_by_its_number_and_reads_within_its_size() {
    assert_eq!(
        path(3),
        "/pulls?state=open&sort=recentupdate&limit=100&page=3"
    );
    assert!(matches!(
        page(b"[]", 1, ("r", "o/r", None)),
        Err(SourceError::TooLarge {
            what: WHAT,
            limit: 1
        })
    ));
}
