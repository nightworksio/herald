use super::*;

#[test]
fn a_github_workflow_run_becomes_a_run_event_with_every_attribute() {
    let payload = json!({
        "action": "completed",
        "workflow_run": workflow_run(9, "completed", Some("failure"), 2),
        "repository": named()
    });
    let (path, event) = delivered_event(
        Forge::GitHub,
        (Delivery::Run, payload.to_string().as_bytes()),
        &RunChoice::default(),
    )
    .unwrap();
    assert_eq!(path, "o/r");
    let event = event.unwrap();
    assert_eq!(event.identity().as_str(), "github.com/7/run/9/2");
    assert_eq!(event.kind(), &Kind::RUN);
    assert_eq!(event.project(), "r");
    assert_eq!(event.title(), "CI failed on main");
    assert_eq!(event.body(), "fix: the thing");
    assert_eq!(
        event.link("run"),
        Some(format!("{PAGE}/actions/runs/9").as_str())
    );
    assert_eq!(event.occurred().to_string(), "2026-11-17T10:00:00Z");
    assert_eq!(event.signal(), Some(Signal::Run));
    let attributes: Vec<(&str, Option<String>)> = [
        ACTION, CONCLUSION, WORKFLOW, TRIGGER, BRANCH, COMMIT, NUMBER, ATTEMPT, ACTOR, FROM,
    ]
    .into_iter()
    .map(|name| (name, text(&event, name)))
    .collect();
    let some = |value: &str| Some(value.to_owned());
    assert_eq!(
        attributes,
        [
            (ACTION, some("completed")),
            (CONCLUSION, some("failure")),
            (WORKFLOW, some("CI")),
            (TRIGGER, some("push")),
            (BRANCH, some("main")),
            (COMMIT, some("abc123")),
            (NUMBER, some("41")),
            (ATTEMPT, some("2")),
            (ACTOR, some("wessel")),
            (FROM, some("actions")),
        ]
    );
    let unfinished = json!({
        "action": "in_progress",
        "workflow_run": workflow_run(9, "in_progress", None, 2),
        "repository": named()
    });
    assert_eq!(
        delivered_event(
            Forge::GitHub,
            (Delivery::Run, unfinished.to_string().as_bytes()),
            &RunChoice::default(),
        )
        .unwrap()
        .1,
        None
    );
}

#[test]
fn a_gitlab_pipeline_hook_becomes_a_run_event_its_merge_requests_read_as_pull_requests() {
    let hook = |status: &str, finished: Value, url: Value| {
        json!({
            "object_kind": "pipeline",
            "object_attributes": {
                "id": 31, "iid": 4, "name": null, "ref": "main", "sha": "def", "status": status,
                "source": "merge_request_event", "url": url,
                "created_at": "2026-11-17 09:00:00 UTC", "finished_at": finished
            },
            "user": {"username": "wessel"},
            "project": {"id": 7, "name": "r", "path_with_namespace": "group/r", "web_url": "https://gitlab.com/group/r"}
        })
    };
    let finished = hook("failed", json!("2026-11-17 09:05:00 UTC"), Value::Null);
    let event = delivered(Forge::GitLab, Delivery::Run, &finished).unwrap();
    assert_eq!(event.identity().as_str(), "gitlab.com/7/run/31/failure");
    assert_eq!(event.title(), "pipeline failed on main");
    assert_eq!(
        event.link("run"),
        Some("https://gitlab.com/group/r/-/pipelines/31")
    );
    assert_eq!(event.occurred().to_string(), "2026-11-17T09:05:00Z");
    assert_eq!(text(&event, TRIGGER).as_deref(), Some("pull_request"));
    assert_eq!(text(&event, NUMBER).as_deref(), Some("4"));
    assert_eq!(text(&event, ACTOR).as_deref(), Some("wessel"));
    let running = hook(
        "running",
        Value::Null,
        json!("https://gitlab.com/group/r/-/pipelines/31?x"),
    );
    let event = delivered(Forge::GitLab, Delivery::Run, &running).unwrap();
    assert_eq!(event.identity().as_str(), "gitlab.com/7/run/31/in_progress");
    assert_eq!(event.occurred().to_string(), "2026-11-17T09:00:00Z");
    assert_eq!(
        event.link("run"),
        Some("https://gitlab.com/group/r/-/pipelines/31?x")
    );
    assert_eq!(
        delivered(
            Forge::GitLab,
            Delivery::Run,
            &hook("unheard-of", Value::Null, Value::Null)
        ),
        None
    );
    let mut timeless = hook("failed", Value::Null, Value::Null);
    timeless["object_attributes"]["created_at"] = Value::Null;
    assert_eq!(
        Forge::GitLab
            .ran(Delivery::Run, timeless.to_string().as_bytes())
            .err(),
        Some(SourceError::Unreadable {
            what: gitlab::RUN,
            reason: "the pipeline has no time".to_owned()
        })
    );
}

#[test]
fn each_gitlab_status_is_read_as_what_the_pipeline_did_and_how_it_ended() {
    let cases = [
        ("created", RunAction::Requested, None),
        ("waiting_for_resource", RunAction::Requested, None),
        ("preparing", RunAction::Requested, None),
        ("pending", RunAction::Requested, None),
        ("scheduled", RunAction::Requested, None),
        ("running", RunAction::InProgress, None),
        ("canceling", RunAction::InProgress, None),
        ("success", RunAction::Completed, Some(Conclusion::Success)),
        ("failed", RunAction::Completed, Some(Conclusion::Failure)),
        (
            "canceled",
            RunAction::Completed,
            Some(Conclusion::Cancelled),
        ),
        ("skipped", RunAction::Completed, Some(Conclusion::Skipped)),
        (
            "manual",
            RunAction::Completed,
            Some(Conclusion::ActionRequired),
        ),
    ];
    for (status, action, conclusion) in cases {
        let listed = json!([{
            "id": 1, "iid": 2, "name": "deploy", "ref": "main", "sha": "s", "status": status,
            "source": "push", "web_url": "https://gitlab.com/g/r/-/pipelines/1",
            "updated_at": "2026-11-17T09:00:00Z"
        }]);
        let found = gitlab::listed(4096, listed.to_string().as_bytes()).unwrap();
        assert_eq!(found.len(), 1, "{status}");
        assert_eq!(
            (found[0].action, found[0].conclusion),
            (action, conclusion),
            "{status}"
        );
        assert_eq!(found[0].workflow, "deploy");
        assert_eq!(found[0].trigger.as_deref(), Some("push"));
    }
    let unknown = json!([{
        "id": 1, "status": "unheard-of", "web_url": "https://gitlab.com/g/r/-/pipelines/1",
        "updated_at": "2026-11-17T09:00:00Z"
    }]);
    assert_eq!(
        gitlab::listed(4096, unknown.to_string().as_bytes()).unwrap(),
        []
    );
}

#[test]
fn a_forgejo_run_event_of_either_generation_becomes_a_run_event() {
    let run = |status: &str, stopped: Value| {
        json!({
            "action": "run_completed",
            "run": {
                "id": 12, "index_in_repo": 5, "workflow_id": "ci.yml", "title": "fix", "status": status,
                "event": "push", "prettyref": "main", "commit_sha": "c0ffee",
                "html_url": "https://codeberg.org/o/r/actions/runs/5",
                "stopped": stopped, "updated": "2026-11-17T08:00:00Z",
                "trigger_user": {"login": "wessel"},
                "repository": {"id": 7, "name": "r", "full_name": "o/r", "html_url": "https://codeberg.org/o/r"}
            }
        })
    };
    let event = delivered(
        Forge::Forgejo,
        Delivery::Run,
        &run("success", json!("2026-11-17T08:03:00Z")),
    )
    .unwrap();
    assert_eq!(event.identity().as_str(), "codeberg.org/7/run/12/success");
    assert_eq!(event.title(), "ci.yml succeeded on main");
    assert_eq!(event.occurred().to_string(), "2026-11-17T08:03:00Z");
    assert_eq!(text(&event, NUMBER).as_deref(), Some("5"));
    let unstopped = delivered(Forge::Forgejo, Delivery::Run, &run("failure", Value::Null)).unwrap();
    assert_eq!(unstopped.occurred().to_string(), "2026-11-17T08:00:00Z");
    let running = delivered(
        Forge::Forgejo,
        Delivery::Run,
        &run("running", json!("2026-11-17T08:03:00Z")),
    )
    .unwrap();
    assert_eq!(running.occurred().to_string(), "2026-11-17T08:00:00Z");
    assert_eq!(
        delivered(Forge::Forgejo, Delivery::Run, &run("unknown", Value::Null)),
        None
    );
    let mut homeless = run("success", Value::Null);
    homeless["run"]["repository"] = Value::Null;
    assert_eq!(
        Forge::Forgejo
            .ran(Delivery::Run, homeless.to_string().as_bytes())
            .err(),
        Some(SourceError::Unreadable {
            what: forgejo::RUN,
            reason: "the run names no repository".to_owned()
        })
    );
    let cases = [
        ("waiting", RunAction::Requested, None),
        ("blocked", RunAction::Requested, None),
        ("running", RunAction::InProgress, None),
        ("success", RunAction::Completed, Some(Conclusion::Success)),
        ("failure", RunAction::Completed, Some(Conclusion::Failure)),
        (
            "cancelled",
            RunAction::Completed,
            Some(Conclusion::Cancelled),
        ),
        ("skipped", RunAction::Completed, Some(Conclusion::Skipped)),
    ];
    for (status, action, conclusion) in cases {
        let listed = json!({"workflow_runs": [run(status, Value::Null)["run"]]});
        let found = forgejo::listed(8192, listed.to_string().as_bytes()).unwrap();
        assert_eq!(
            (found[0].action, found[0].conclusion),
            (action, conclusion),
            "{status}"
        );
    }
}

pub(super) fn suite(slug: &str, status: &str, conclusion: Option<&str>) -> Value {
    json!({
        "id": 55, "head_branch": "main", "head_sha": "abc", "status": status, "conclusion": conclusion,
        "app": {"slug": slug, "name": "Buildkite"}, "updated_at": "2026-11-17T11:00:00Z"
    })
}

#[test]
fn a_check_suite_of_another_ci_is_a_run_from_checks_and_github_actions_own_is_left_out() {
    let payload = json!({"action": "completed", "check_suite": suite("buildkite", "completed", Some("timed_out")), "repository": named()});
    let event = delivered(Forge::GitHub, Delivery::CheckSuite, &payload).unwrap();
    assert_eq!(
        event.identity().as_str(),
        "github.com/7/check-suite/55/timed_out"
    );
    assert_eq!(event.title(), "Buildkite timed out on main");
    assert_eq!(
        event.link("run"),
        Some(format!("{PAGE}/commit/abc/checks?check_suite_id=55").as_str())
    );
    assert_eq!(text(&event, FROM).as_deref(), Some("checks"));
    let own = json!({"action": "completed", "check_suite": suite("github-actions", "completed", Some("success")), "repository": named()});
    assert_eq!(delivered(Forge::GitHub, Delivery::CheckSuite, &own), None);
    let unpaged = json!({"check_suite": suite("buildkite", "completed", None), "repository": {"id": 7, "name": "r", "full_name": "o/r"}});
    assert!(matches!(
        Forge::GitHub.ran(Delivery::CheckSuite, unpaged.to_string().as_bytes()),
        Err(SourceError::Unreadable { reason, .. }) if reason == "it names no web page of the repository"
    ));
    let listed = json!({"check_suites": [suite("buildkite", "in_progress", None), suite("github-actions", "completed", None)]});
    let found = github::listed_suites(8192, &repository(), listed.to_string().as_bytes()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].action, RunAction::InProgress);
    let homeless = Repository {
        page: None,
        ..repository()
    };
    assert!(github::listed_suites(8192, &homeless, listed.to_string().as_bytes()).is_err());
}

#[test]
fn a_commit_status_is_a_run_from_checks_linking_its_target_or_its_commit() {
    let status = |state: &str, target: Value, sha: Value| {
        json!({
            "id": 66, "sha": sha, "state": state, "context": "ci/circleci", "description": "Tests passed",
            "target_url": target, "updated_at": "2026-11-17T12:00:00Z",
            "branches": [{"name": "main"}], "sender": {"login": "circleci"}, "repository": named()
        })
    };
    let targeted = delivered(
        Forge::GitHub,
        Delivery::Status,
        &status("success", json!("https://circleci.com/b/1"), json!("abc")),
    )
    .unwrap();
    assert_eq!(
        targeted.identity().as_str(),
        "github.com/7/status/66/success"
    );
    assert_eq!(targeted.title(), "ci/circleci succeeded on main");
    assert_eq!(targeted.body(), "Tests passed");
    assert_eq!(targeted.link("run"), Some("https://circleci.com/b/1"));
    assert_eq!(text(&targeted, ACTOR).as_deref(), Some("circleci"));
    assert_eq!(text(&targeted, COMMIT).as_deref(), Some("abc"));
    let commit = delivered(
        Forge::GitHub,
        Delivery::Status,
        &status("error", json!("javascript:x"), json!("abc")),
    )
    .unwrap();
    assert_eq!(
        commit.link("run"),
        Some(format!("{PAGE}/commit/abc").as_str())
    );
    assert_eq!(text(&commit, CONCLUSION).as_deref(), Some("failure"));
    let pending = delivered(
        Forge::GitHub,
        Delivery::Status,
        &status("pending", Value::Null, Value::Null),
    )
    .unwrap();
    assert_eq!(pending.link("run"), Some(PAGE));
    assert_eq!(
        pending.identity().as_str(),
        "github.com/7/status/66/in_progress"
    );
    let failed = delivered(
        Forge::GitHub,
        Delivery::Status,
        &status("failure", Value::Null, json!("abc")),
    )
    .unwrap();
    assert_eq!(text(&failed, CONCLUSION).as_deref(), Some("failure"));
    let listed = json!([{"id": 67, "state": "success", "context": null, "description": null, "target_url": null,
        "updated_at": "2026-11-17T12:00:00Z", "creator": {"login": "bot"}}]);
    let found = github::listed_statuses(
        8192,
        (&repository(), Some("dev".to_owned())),
        listed.to_string().as_bytes(),
    )
    .unwrap();
    assert_eq!(found[0].workflow, "status");
    assert_eq!(found[0].branch.as_deref(), Some("dev"));
    assert_eq!(found[0].actor.as_deref(), Some("bot"));
    let homeless = Repository {
        page: None,
        ..repository()
    };
    assert!(
        github::listed_statuses(8192, (&homeless, None), listed.to_string().as_bytes()).is_err()
    );
    let unpaged = json!({"id": 1, "state": "success", "updated_at": "2026-11-17T12:00:00Z", "repository": {"id": 7, "name": "r", "full_name": "o/r"}});
    assert!(
        Forge::GitHub
            .ran(Delivery::Status, unpaged.to_string().as_bytes())
            .is_err()
    );
}
