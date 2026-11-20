use serde_json::{Value, json};

use super::*;

fn node(fields: &Value) -> Value {
    let mut node = json!({
        "number": 4, "title": "Bump serde", "url": "https://github.com/o/r/pull/4",
        "isDraft": false, "createdAt": "2026-11-18T10:00:00Z", "updatedAt": "2026-11-18T11:00:00Z"
    });
    for (key, value) in fields.as_object().unwrap() {
        node[key] = value.clone();
    }
    node
}

fn read(visibility: Option<&str>, nodes: &[Value], known: Option<Visibility>) -> Page {
    let body = json!({ "data": { "repository": { "visibility": visibility, "pullRequests": { "nodes": nodes } } } });
    page(body.to_string().as_bytes(), 1 << 20, ("r", "o/r", known)).unwrap()
}

fn one(fields: &Value) -> Pull {
    read(Some("PUBLIC"), &[node(fields)], None).pulls.remove(0)
}

fn rollup(state: &str, contexts: &Value) -> Value {
    json!({ "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": state, "contexts": { "nodes": contexts } } } } ] } })
}

#[test]
fn a_bare_node_reads_as_an_unasked_unchecked_pull_of_its_repository() {
    let pull = one(&json!({}));
    assert_eq!(pull.project, "r");
    assert_eq!(pull.repository, "o/r");
    assert_eq!(pull.visibility, Visibility::Public);
    assert_eq!(pull.number, 4);
    assert_eq!(pull.title, "Bump serde");
    assert_eq!(pull.url, "https://github.com/o/r/pull/4");
    assert_eq!(pull.author, None);
    assert!(!pull.bot && !pull.draft && !pull.queued);
    assert_eq!(pull.labels, Vec::<String>::new());
    assert_eq!(pull.opened.to_string(), "2026-11-18T10:00:00Z");
    assert_eq!(pull.updated.to_string(), "2026-11-18T11:00:00Z");
    assert_eq!(pull.review, Review::Unasked);
    assert_eq!(pull.checks, Checks::Absent);
    assert_eq!(pull.mergeable, Mergeable::Unknown);
    assert_eq!(pull.forge, None);
}

#[test]
fn authors_labels_and_drafts_are_read_and_bots_told_by_kind_or_login() {
    let pull = one(&json!({
        "isDraft": true,
        "author": { "__typename": "User", "login": "ann" },
        "labels": { "nodes": [ { "name": "a" }, { "name": "b" } ] }
    }));
    assert_eq!(pull.author.as_deref(), Some("ann"));
    assert!(pull.draft && !pull.bot);
    assert_eq!(pull.labels, ["a", "b"]);
    let app = one(&json!({ "author": { "__typename": "Bot", "login": "dependabot" } }));
    assert!(app.bot);
    let named = one(&json!({ "author": { "__typename": "User", "login": "x[bot]" } }));
    assert!(named.bot);
}

#[test]
fn the_review_decision_and_requests_tell_the_review() {
    let review = |fields: Value| one(&fields).review;
    assert_eq!(
        review(json!({ "reviewDecision": "APPROVED" })),
        Review::Approved
    );
    assert_eq!(
        review(json!({ "reviewDecision": "CHANGES_REQUESTED" })),
        Review::ChangesRequested
    );
    assert_eq!(
        review(
            json!({ "reviewDecision": "REVIEW_REQUIRED", "reviewRequests": { "totalCount": 1 } })
        ),
        Review::Requested
    );
    assert_eq!(
        review(
            json!({ "reviewDecision": "REVIEW_REQUIRED", "reviewRequests": { "totalCount": 0 } })
        ),
        Review::Unasked
    );
    assert_eq!(
        review(json!({ "reviewDecision": "APPROVED", "reviewRequests": { "totalCount": 2 } })),
        Review::Approved
    );
    let asked = one(&json!({ "reviewRequests": { "totalCount": 4, "nodes": [
        { "requestedReviewer": { "login": "ann" } },
        { "requestedReviewer": { "combinedSlug": "o/core" } },
        { "requestedReviewer": {} },
        { "requestedReviewer": null }
    ] } }));
    assert_eq!(asked.reviewers, ["ann", "o/core"]);
    assert_eq!(one(&json!({})).reviewers, Vec::<String>::new());
}

#[test]
fn mergeability_and_the_merge_state_are_read_and_a_queue_wins() {
    let pull = |fields: Value| one(&fields);
    assert_eq!(
        pull(json!({ "mergeable": "MERGEABLE" })).mergeable,
        Mergeable::Clean
    );
    assert_eq!(
        pull(json!({ "mergeable": "CONFLICTING" })).mergeable,
        Mergeable::Conflict
    );
    assert_eq!(
        pull(json!({ "mergeable": "UNKNOWN" })).mergeable,
        Mergeable::Unknown
    );
    let turns: Vec<(Turn, String)> = [
        "CLEAN",
        "HAS_HOOKS",
        "UNSTABLE",
        "BLOCKED",
        "BEHIND",
        "DIRTY",
        "DRAFT",
        "UNKNOWN",
    ]
    .iter()
    .map(|status| pull(json!({ "mergeStateStatus": status })).forge.unwrap())
    .collect();
    let expected = [
        Turn::WaitingOnMerge,
        Turn::WaitingOnMerge,
        Turn::WaitingOnMerge,
        Turn::WaitingOnReview,
        Turn::WaitingOnAuthor,
        Turn::WaitingOnAuthor,
        Turn::Draft,
        Turn::Unknown,
    ];
    for ((turn, status), (wanted, named)) in turns.iter().zip(expected.iter().zip([
        "CLEAN",
        "HAS_HOOKS",
        "UNSTABLE",
        "BLOCKED",
        "BEHIND",
        "DIRTY",
        "DRAFT",
        "UNKNOWN",
    ])) {
        assert_eq!((turn, status.as_str()), (wanted, named));
    }
    let queued =
        pull(json!({ "isInMergeQueue": true, "mergeStateStatus": "CLEAN", "headRefOid": "abc" }));
    assert!(queued.queued && !queued.unread);
    assert_eq!(queued.head.as_deref(), Some("abc"));
    assert_eq!(
        queued.forge,
        Some((Turn::Queued, "IN_MERGE_QUEUE".to_owned()))
    );
}

#[test]
fn the_head_s_rollup_tells_the_checks_naming_each_that_failed() {
    let checks = |fields: Value| one(&fields).checks;
    assert_eq!(checks(rollup("SUCCESS", &json!([]))), Checks::Passed);
    assert_eq!(checks(rollup("PENDING", &json!([]))), Checks::Pending);
    assert_eq!(checks(rollup("EXPECTED", &json!([]))), Checks::Pending);
    let contexts = json!([
        { "__typename": "CheckRun", "name": "test", "conclusion": "FAILURE" },
        { "__typename": "CheckRun", "name": "lint", "conclusion": "SUCCESS" },
        { "__typename": "CheckRun", "name": "slow", "conclusion": "TIMED_OUT" },
        { "__typename": "CheckRun", "name": "stop", "conclusion": "CANCELLED" },
        { "__typename": "CheckRun", "name": "act", "conclusion": "ACTION_REQUIRED" },
        { "__typename": "CheckRun", "name": "boot", "conclusion": "STARTUP_FAILURE" },
        { "__typename": "CheckRun", "name": "running", "conclusion": null },
        { "__typename": "StatusContext", "context": "ci/legacy", "state": "ERROR" },
        { "__typename": "StatusContext", "context": "ci/red", "state": "FAILURE" },
        { "__typename": "StatusContext", "context": "ci/ok", "state": "SUCCESS" }
    ]);
    let failed =
        |names: &[&str]| Checks::Failed(names.iter().map(|name| (*name).to_owned()).collect());
    assert_eq!(
        checks(rollup("FAILURE", &contexts)),
        failed(&["test", "slow", "stop", "act", "boot", "ci/legacy", "ci/red"])
    );
    assert_eq!(checks(rollup("ERROR", &json!([]))), failed(&[]));
    assert_eq!(
        checks(json!({ "commits": { "nodes": [] } })),
        Checks::Absent
    );
    assert_eq!(
        checks(json!({ "commits": { "nodes": [ { "commit": { "statusCheckRollup": null } } ] } })),
        Checks::Absent
    );
}

#[test]
fn the_repository_s_visibility_is_read_unless_its_source_knows_it() {
    let seen = |named: Option<&str>, known: Option<Visibility>| {
        read(named, &[node(&json!({}))], known).pulls[0].visibility
    };
    assert_eq!(seen(Some("PUBLIC"), None), Visibility::Public);
    assert_eq!(seen(Some("INTERNAL"), None), Visibility::Internal);
    assert_eq!(seen(Some("PRIVATE"), None), Visibility::Private);
    assert_eq!(seen(None, None), Visibility::Private);
    assert_eq!(
        seen(Some("PUBLIC"), Some(Visibility::Private)),
        Visibility::Private
    );
}

#[test]
fn a_page_names_the_next_only_where_one_follows() {
    let paged = |info: Value| {
        let body = json!({ "data": { "repository": { "pullRequests": { "pageInfo": info, "nodes": [] } } } });
        page(body.to_string().as_bytes(), 1 << 20, ("r", "o/r", None))
            .unwrap()
            .next
    };
    assert_eq!(
        paged(json!({ "hasNextPage": true, "endCursor": "c" })).as_deref(),
        Some("c")
    );
    assert_eq!(
        paged(json!({ "hasNextPage": false, "endCursor": "c" })),
        None
    );
}

#[test]
fn an_answer_without_its_repository_or_over_its_size_does_not_read() {
    let unread =
        |body: Value, parse: usize| page(body.to_string().as_bytes(), parse, ("r", "o/r", None));
    assert!(matches!(
        unread(json!({ "data": { "repository": null } }), 1 << 20),
        Err(SourceError::Unreadable { what: WHAT, .. })
    ));
    assert!(matches!(
        unread(json!({ "errors": [] }), 1 << 20),
        Err(SourceError::Unreadable { what: WHAT, .. })
    ));
    assert!(matches!(
        unread(json!({ "data": null }), 4),
        Err(SourceError::TooLarge {
            what: WHAT,
            limit: 4
        })
    ));
}

#[test]
fn a_page_is_asked_of_the_repository_after_its_cursor() {
    let ask = Ask {
        parse: 9,
        files: false,
    };
    let asked = request("https://ghe.example/api/graphql", "o/r", Some("c1"), ask);
    assert_eq!(asked.url.expose(), "https://ghe.example/api/graphql");
    assert_eq!(asked.most, 9);
    let body: Value = serde_json::from_slice(&asked.body).unwrap();
    assert_eq!(
        body["variables"],
        json!({ "owner": "o", "name": "r", "after": "c1" })
    );
    let query = body["query"].as_str().unwrap();
    assert!(query.contains("pullRequests(states: OPEN, first: 100, after: $after"));
    assert!(query.contains("labels(first: 50)"));
    assert!(query.contains("baseRefName"));
    assert!(!query.contains("files("));
    let first = request("https://api.github.com/graphql", "o", None, ask);
    let body: Value = serde_json::from_slice(&first.body).unwrap();
    assert_eq!(
        body["variables"],
        json!({ "owner": "o", "name": "", "after": null })
    );
}

#[test]
fn a_view_predicting_conflicts_asks_each_pull_request_s_changed_paths_and_its_branch() {
    let ask = Ask {
        parse: 9,
        files: true,
    };
    for asked in [
        request("https://api.github.com/graphql", "o/r", None, ask),
        numbered_request("https://api.github.com/graphql", "o/r", &[1], ask),
        scoped_request("https://api.github.com/graphql", &[("o/r", vec![1])], ask),
    ] {
        let body: Value = serde_json::from_slice(&asked.body).unwrap();
        let query = body["query"].as_str().unwrap();
        assert!(
            query.contains("files(first: 100) { totalCount nodes { path } }"),
            "{query}"
        );
    }
    let read = one(&json!({ "baseRefName": "main",
        "files": { "totalCount": 3, "nodes": [{ "path": "a.rs" }, { "path": "b.rs" }] } }));
    assert_eq!(read.base.as_deref(), Some("main"));
    assert_eq!(
        read.files,
        Some(Changed {
            paths: ["a.rs".to_owned(), "b.rs".to_owned()].into(),
            whole: false
        })
    );
    let whole = one(&json!({ "files": { "totalCount": 1, "nodes": [{ "path": "a.rs" }] } }));
    assert!(whole.files.unwrap().whole);
    assert_eq!(one(&json!({})).files, None);
}
