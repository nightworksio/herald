use serde_json::{Value, json};

use super::*;

fn on(forge: Forge, body: &Value, requested: bool) -> Review {
    reviews(forge, body.to_string().as_bytes(), 1 << 20, requested).unwrap()
}

fn review(body: &Value, requested: bool) -> Review {
    on(Forge::GitHub, body, requested)
}

fn given(login: &str, state: &str) -> Value {
    json!({ "user": { "login": login }, "state": state, "author_association": "MEMBER" })
}

fn by(association: &str, state: &str) -> Value {
    json!({ "user": { "login": "eve" }, "state": state, "author_association": association })
}

#[test]
fn each_reviewer_s_last_verdict_stands_and_a_change_asked_outweighs_an_approval() {
    assert_eq!(review(&json!([]), false), Review::Unasked);
    assert_eq!(review(&json!([]), true), Review::Requested);
    assert_eq!(
        review(&json!([given("ann", "APPROVED")]), true),
        Review::Approved
    );
    assert_eq!(
        review(
            &json!([given("ann", "APPROVED"), given("bob", "CHANGES_REQUESTED")]),
            false
        ),
        Review::ChangesRequested
    );
    assert_eq!(
        review(&json!([given("ann", "REQUEST_CHANGES")]), false),
        Review::ChangesRequested
    );
    assert_eq!(
        review(
            &json!([given("ann", "CHANGES_REQUESTED"), given("ann", "APPROVED")]),
            false
        ),
        Review::Approved
    );
    assert_eq!(
        review(
            &json!([given("ann", "APPROVED"), given("ann", "COMMENTED")]),
            false
        ),
        Review::Approved
    );
}

#[test]
fn a_dismissed_review_or_one_by_nobody_says_nothing() {
    assert_eq!(
        review(
            &json!([given("ann", "APPROVED"), given("ann", "DISMISSED")]),
            true
        ),
        Review::Requested
    );
    assert_eq!(
        on(
            Forge::Forgejo,
            &json!([ { "user": { "login": "ann" }, "state": "APPROVED", "dismissed": true } ]),
            false
        ),
        Review::Unasked
    );
    assert_eq!(
        review(&json!([ { "user": null, "state": "APPROVED" } ]), false),
        Review::Unasked
    );
}

#[test]
fn only_a_trusted_reviewer_s_verdict_decides_on_github() {
    for association in ["OWNER", "MEMBER", "COLLABORATOR"] {
        assert_eq!(
            review(&json!([by(association, "APPROVED")]), false),
            Review::Approved
        );
    }
    for association in ["CONTRIBUTOR", "FIRST_TIME_CONTRIBUTOR", "NONE"] {
        assert_eq!(
            review(&json!([by(association, "APPROVED")]), true),
            Review::Requested
        );
        assert_eq!(
            review(&json!([by(association, "CHANGES_REQUESTED")]), false),
            Review::Unasked
        );
    }
    assert_eq!(
        review(
            &json!([given("ann", "APPROVED"), by("NONE", "CHANGES_REQUESTED")]),
            false
        ),
        Review::Approved
    );
}

#[test]
fn only_an_official_review_decides_on_forgejo() {
    let official = |official: bool| json!([ { "user": { "login": "ann" }, "state": "APPROVED", "official": official } ]);
    assert_eq!(on(Forge::Forgejo, &official(true), false), Review::Approved);
    assert_eq!(on(Forge::Forgejo, &official(false), false), Review::Unasked);
}

#[test]
fn a_verdict_whose_reviewer_s_standing_is_unread_leaves_the_review_unknown() {
    let bare = json!([ { "user": { "login": "ann" }, "state": "APPROVED" } ]);
    assert_eq!(review(&bare, false), Review::Unknown);
    assert_eq!(on(Forge::Forgejo, &bare, false), Review::Unknown);
    let comment = json!([ { "user": { "login": "ann" }, "state": "COMMENTED" } ]);
    assert_eq!(review(&comment, true), Review::Requested);
    let dismissed =
        json!([given("ann", "APPROVED"), { "user": { "login": "ann" }, "state": "DISMISSED" }]);
    assert_eq!(review(&dismissed, false), Review::Unasked);
}

fn github(runs: &Value) -> Checks {
    let body = json!({ "check_runs": runs }).to_string();
    checks(Forge::GitHub, body.as_bytes(), 1 << 20).unwrap()
}

fn run(name: &str, status: &str, conclusion: Option<&str>) -> Value {
    json!({ "name": name, "status": status, "conclusion": conclusion })
}

#[test]
fn github_s_check_runs_tell_the_checks_naming_each_that_failed() {
    assert_eq!(github(&json!([])), Checks::Absent);
    assert_eq!(
        checks(Forge::GitHub, b"{}", 1 << 20).unwrap(),
        Checks::Absent
    );
    assert_eq!(
        github(&json!([run("test", "completed", Some("success"))])),
        Checks::Passed
    );
    assert_eq!(
        github(&json!([
            run("test", "completed", Some("success")),
            run("lint", "in_progress", None)
        ])),
        Checks::Pending
    );
    assert_eq!(
        github(&json!([
            run("test", "completed", Some("timed_out")),
            run("lint", "queued", None),
            run("doc", "completed", Some("neutral"))
        ])),
        Checks::Failed(vec!["test".to_owned()])
    );
}

fn forgejo(statuses: &Value) -> Checks {
    let body = json!({ "state": "ignored", "statuses": statuses }).to_string();
    checks(Forge::Forgejo, body.as_bytes(), 1 << 20).unwrap()
}

fn status(context: &str, state: &str) -> Value {
    json!({ "context": context, "status": state })
}

#[test]
fn forgejo_s_combined_status_tells_the_checks_naming_each_that_failed() {
    assert_eq!(forgejo(&json!([])), Checks::Absent);
    assert_eq!(
        forgejo(&json!([status("ci", "success"), status("lint", "warning")])),
        Checks::Passed
    );
    assert_eq!(
        forgejo(&json!([status("ci", "success"), status("lint", "pending")])),
        Checks::Pending
    );
    assert_eq!(
        forgejo(&json!([status("ci", "failure"), status("lint", "error")])),
        Checks::Failed(vec!["ci".to_owned(), "lint".to_owned()])
    );
}

#[test]
fn each_detail_is_asked_by_its_path_and_reads_within_its_size() {
    assert_eq!(
        reviews_path(Forge::GitHub, 7),
        "/pulls/7/reviews?per_page=100"
    );
    assert_eq!(
        reviews_path(Forge::Forgejo, 7),
        "/pulls/7/reviews?limit=100"
    );
    assert_eq!(
        checks_path(Forge::GitHub, "abc"),
        "/commits/abc/check-runs?per_page=100"
    );
    assert_eq!(
        checks_path(Forge::Forgejo, "abc"),
        "/commits/abc/status?limit=100"
    );
    assert!(matches!(
        reviews(Forge::GitHub, b"[]", 1, false),
        Err(SourceError::TooLarge {
            what: REVIEWS,
            limit: 1
        })
    ));
    assert!(matches!(
        checks(Forge::Forgejo, b"{}", 1),
        Err(SourceError::TooLarge {
            what: CHECKS,
            limit: 1
        })
    ));
}

#[test]
fn a_pull_request_s_changed_paths_are_read_with_each_path_it_changes_from() {
    let listed = json!([
        { "filename": "src/new.rs", "previous_filename": "src/old.rs" },
        { "new_path": "docs/a.md", "old_path": "docs/a.md" },
        { "filename": "README.md" }
    ]);
    let read = files(listed.to_string().as_bytes(), 1 << 20).unwrap();
    let paths: Vec<&str> = read.paths.iter().map(String::as_str).collect();
    assert_eq!(
        paths,
        ["README.md", "docs/a.md", "src/new.rs", "src/old.rs"]
    );
    assert!(read.whole);
    let full: Vec<Value> = (0..PAGE)
        .map(|place| json!({ "filename": format!("f{place}") }))
        .collect();
    let partial = files(Value::from(full).to_string().as_bytes(), 1 << 20).unwrap();
    assert!(!partial.whole);
    assert!(files(b"{}", 1 << 20).is_err());
    assert_eq!(
        files_path(Forge::GitLab, 4),
        "/merge_requests/4/diffs?per_page=100"
    );
    assert_eq!(files_path(Forge::GitHub, 4), "/pulls/4/files?per_page=100");
    assert_eq!(files_path(Forge::Forgejo, 4), "/pulls/4/files?limit=100");
}
