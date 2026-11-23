use super::forges::suite;
use super::*;

#[test]
fn a_poll_asks_each_forge_for_only_the_runs_one_parameter_can_name() {
    let narrow = choice(RunsFile {
        conclusions: vec![Conclusion::Failure],
        branches: vec!["main".to_owned()],
        triggers: vec!["pull_request".to_owned()],
        ..RunsFile::default()
    });
    assert_eq!(
        github::runs_path(&narrow, 20),
        "/actions/runs?per_page=20&status=failure&event=pull_request&branch=main"
    );
    assert_eq!(
        gitlab::runs_path(&narrow, 20),
        "/pipelines?per_page=20&order_by=updated_at&sort=desc&scope=finished&source=merge_request_event&ref=main"
    );
    assert_eq!(
        forgejo::runs_path(&narrow, 20),
        "/actions/runs?limit=20&status=failure&event=pull_request&ref=refs%2Fheads%2Fmain"
    );
    let finished = RunChoice::default();
    assert_eq!(
        github::runs_path(&finished, 5),
        "/actions/runs?per_page=5&status=completed"
    );
    assert_eq!(forgejo::runs_path(&finished, 5), "/actions/runs?limit=5");
    let wide = choice(RunsFile {
        actions: Some(vec![RunAction::Requested, RunAction::Completed]),
        branches: vec!["release/*".to_owned()],
        triggers: vec!["push".to_owned(), "schedule".to_owned()],
        ..RunsFile::default()
    });
    assert_eq!(github::runs_path(&wide, 5), "/actions/runs?per_page=5");
    assert_eq!(
        gitlab::runs_path(&wide, 5),
        "/pipelines?per_page=5&order_by=updated_at&sort=desc"
    );
    for (action, scope) in [
        (RunAction::InProgress, "running"),
        (RunAction::Requested, "pending"),
    ] {
        let one = choice(RunsFile {
            actions: Some(vec![action]),
            triggers: vec!["web".to_owned()],
            ..RunsFile::default()
        });
        assert_eq!(
            gitlab::runs_path(&one, 5),
            format!("/pipelines?per_page=5&order_by=updated_at&sort=desc&scope={scope}&source=web")
        );
        assert_eq!(
            github::runs_path(&one, 5),
            format!(
                "/actions/runs?per_page=5&status={}&event=web",
                action.name()
            )
        );
    }
    assert_eq!(
        github::checks_paths("release/1", 9).map(|(path, ran)| (path, ran.name())),
        [
            (
                "/commits/release/1/check-suites?per_page=9".to_owned(),
                "check-suite"
            ),
            (
                "/commits/release/1/statuses?per_page=9".to_owned(),
                "status"
            ),
        ]
    );
}

fn asking_polled(
    forge: Forge,
    choice: &RunChoice,
    (http, cursor): (&ScriptedHttp, Option<&Cursor>),
) -> Result<(bool, Found), Unpolled> {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    let origin = Origin::new(forge, &options, "o/r");
    let asking = Asking {
        origin: &origin,
        token: None,
        choice,
    };
    let http = Erased(http as &dyn DynHttp);
    let asked = pollster::block_on(asking.asked(&http, cursor))?;
    let runs = asking.read(&asked, &repository())?;
    Ok((asked.changed(), runs))
}

#[test]
fn a_poll_reads_each_changed_listing_and_keeps_the_validators_of_each() {
    let both = choice(RunsFile {
        from: Some(vec![RunFrom::Actions, RunFrom::Checks]),
        ..RunsFile::default()
    });
    let http = ScriptedHttp::new(vec![
        answer(
            200,
            &json!({"workflow_runs": [workflow_run(9, "completed", Some("success"), 1), workflow_run(8, "in_progress", None, 1)]}),
            Some("\"r\""),
        ),
        answer(
            200,
            &json!({"check_suites": [suite("buildkite", "completed", Some("failure"))]}),
            Some("\"c\""),
        ),
        answer(304, &Value::Null, None),
    ]);
    let kept = Cursor {
        listings: BTreeMap::from([(
            "status".to_owned(),
            Validators {
                etag: Some("\"s\"".to_owned()),
                last_modified: None,
            },
        )]),
        ..Cursor::default()
    };
    let (changed, runs) = asking_polled(Forge::GitHub, &both, (&http, Some(&kept))).unwrap();
    assert!(changed);
    let identities: Vec<&str> = runs
        .events
        .iter()
        .map(|event| event.identity().as_str())
        .collect();
    assert_eq!(
        identities,
        [
            "github.com/7/run/9/1",
            "github.com/7/check-suite/55/failure"
        ]
    );
    assert_eq!(runs.validators.unwrap().etag.as_deref(), Some("\"r\""));
    assert_eq!(runs.listings["check-suite"].etag.as_deref(), Some("\"c\""));
    assert_eq!(runs.listings["status"].etag.as_deref(), Some("\"s\""));
    let asked: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        asked,
        [
            "https://api.github.com/repos/o/r/actions/runs?per_page=30&status=completed",
            "https://api.github.com/repos/o/r/commits/HEAD/check-suites?per_page=30",
            "https://api.github.com/repos/o/r/commits/HEAD/statuses?per_page=30",
        ]
    );
    assert_eq!(http.asked()[2].1.as_deref(), Some("\"s\""));
    let quiet = ScriptedHttp::new(vec![answer(304, &Value::Null, None)]);
    let (changed, runs) =
        asking_polled(Forge::GitLab, &RunChoice::default(), (&quiet, None)).unwrap();
    assert!(!changed);
    assert_eq!(runs.events, []);
    assert_eq!(runs.validators, Some(Validators::default()));
    let unreadable = ScriptedHttp::new(vec![answer(500, &json!({}), None)]);
    assert!(matches!(
        asking_polled(Forge::Forgejo, &RunChoice::default(), (&unreadable, None)),
        Err(Unpolled::Unreadable(_))
    ));
    let limited = ScriptedHttp::new(vec![answer(429, &json!({}), None)]);
    assert!(matches!(
        asking_polled(Forge::GitLab, &RunChoice::default(), (&limited, None)),
        Err(Unpolled::RateLimited(_))
    ));
    let unreachable = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        asking_polled(Forge::GitHub, &RunChoice::default(), (&unreachable, None)),
        Err(Unpolled::Unreachable(_))
    ));
    let unlinked = ScriptedHttp::new(vec![answer(
        200,
        &json!({"workflow_runs": [{"id": 1, "html_url": "x", "updated_at": "2026-11-17T10:00:00Z", "status": "completed"}]}),
        None,
    )]);
    assert!(matches!(
        asking_polled(Forge::GitHub, &RunChoice::default(), (&unlinked, None)),
        Err(Unpolled::Unreadable(_))
    ));
}

#[test]
fn runs_merge_into_a_poll_keeping_the_cursor_s_validators_where_the_rest_found_nothing() {
    let event = |id: u64, at: &str| {
        ForgeRun {
            ran: Ran::Run,
            id,
            attempt: Some(1),
            number: None,
            workflow: "CI".to_owned(),
            action: RunAction::Completed,
            conclusion: None,
            trigger: None,
            branch: None,
            commit: None,
            actor: None,
            url: PAGE.to_owned(),
            title: None,
            occurred: at.parse().unwrap(),
        }
        .event("runs", &repository())
        .unwrap()
    };
    let runs = || Found {
        events: vec![event(1, "2026-11-17T09:00:00Z")],
        validators: Some(Validators {
            etag: Some("r".to_owned()),
            last_modified: None,
        }),
        listings: BTreeMap::from([("status".to_owned(), Validators::default())]),
    };
    let cursor = Cursor {
        etag: Some("releases".to_owned()),
        last_modified: Some("Tue, 17 Nov 2026 08:00:00 GMT".to_owned()),
        signals: BTreeMap::from([(Signal::Tag, Validators::default())]),
        packages: BTreeMap::from([("p".to_owned(), Validators::default())]),
        listings: BTreeMap::from([("check-suite".to_owned(), Validators::default())]),
        ..Cursor::default()
    };
    let changed = Asked(vec![(Ran::Run, None, answer(200, &json!({}), None))]);
    let unchanged = Asked(vec![(Ran::Run, None, answer(304, &Value::Null, None))]);
    assert_eq!(
        merged_into(None, (unchanged.changed(), runs()), Some(&cursor)),
        None
    );
    let kept = merged_into(None, (changed.changed(), runs()), Some(&cursor)).unwrap();
    assert_eq!(kept.etag.as_deref(), Some("releases"));
    assert!(kept.signals.contains_key(&Signal::Tag));
    assert_eq!(kept.signals[&Signal::Run].etag.as_deref(), Some("r"));
    assert!(kept.packages.contains_key("p"));
    assert!(kept.listings.contains_key("status"));
    assert!(kept.listings.contains_key("check-suite"));
    assert_eq!(
        kept.last_modified.as_deref(),
        Some("Tue, 17 Nov 2026 08:00:00 GMT")
    );
    let found = Polled {
        events: vec![event(2, "2026-11-17T10:00:00Z")],
        ..Polled::default()
    };
    let both = merged_into(Some(found), (unchanged.changed(), runs()), None).unwrap();
    let identities: Vec<&Identity> = both.events.iter().map(Event::identity).collect();
    assert_eq!(
        identities,
        [
            &Identity::new("github.com/7/run/2/1"),
            &Identity::new("github.com/7/run/1/1")
        ]
    );
    let bare = merged_into(None, (changed.changed(), Found::default()), None).unwrap();
    assert!(!bare.signals.contains_key(&Signal::Run));
}

#[test]
fn a_poll_reads_gitlab_s_pipelines_github_s_statuses_and_forgejo_s_unnamed_runs() {
    let pipelines = ScriptedHttp::new(vec![answer(
        200,
        &json!([{
            "id": 31, "status": "failed", "web_url": "https://gitlab.com/o/r/-/pipelines/31",
            "updated_at": "2026-11-17T09:00:00Z"
        }]),
        None,
    )]);
    let (_, runs) =
        asking_polled(Forge::GitLab, &RunChoice::default(), (&pipelines, None)).unwrap();
    let identities: Vec<&str> = runs
        .events
        .iter()
        .map(|event| event.identity().as_str())
        .collect();
    assert_eq!(identities, ["github.com/7/run/31/failure"]);
    let statuses = choice(RunsFile {
        from: Some(vec![RunFrom::Checks]),
        branches: vec!["main".to_owned()],
        ..RunsFile::default()
    });
    let http = ScriptedHttp::new(vec![
        answer(200, &json!({"check_suites": []}), None),
        answer(
            200,
            &json!([{"id": 70, "state": "failure", "updated_at": "2026-11-17T12:00:00Z"}]),
            Some("\"s\""),
        ),
    ]);
    let (_, runs) = asking_polled(Forge::GitHub, &statuses, (&http, None)).unwrap();
    assert_eq!(
        runs.events[0].identity().as_str(),
        "github.com/7/status/70/failure"
    );
    assert_eq!(runs.events[0].title(), "status failed on main");
    assert_eq!(runs.listings["status"].etag.as_deref(), Some("\"s\""));
    assert_eq!(runs.validators, None);
    assert_eq!(
        http.asked()[1].0,
        "https://api.github.com/repos/o/r/commits/main/statuses?per_page=30"
    );
    let unnamed = ScriptedHttp::new(vec![answer(
        200,
        &json!({"workflow_runs": [{
            "id": 12, "status": "success", "html_url": "https://codeberg.org/o/r/actions/runs/5",
            "updated": "2026-11-17T08:00:00Z"
        }]}),
        None,
    )]);
    let (_, runs) = asking_polled(Forge::Forgejo, &RunChoice::default(), (&unnamed, None)).unwrap();
    assert_eq!(runs.events[0].title(), "workflow succeeded");
}

#[test]
fn a_listing_that_cannot_be_read_is_named_in_the_poll_s_reason() {
    let refused = || answer(500, &json!({}), None);
    let checks = choice(RunsFile {
        from: Some(vec![RunFrom::Checks]),
        ..RunsFile::default()
    });
    let cases = [
        (
            Forge::GitHub,
            RunChoice::default(),
            vec![refused()],
            github::RUNS,
        ),
        (
            Forge::GitHub,
            checks.clone(),
            vec![refused(), refused()],
            github::SUITES,
        ),
        (
            Forge::GitHub,
            checks,
            vec![answer(304, &Value::Null, None), refused()],
            github::STATUSES,
        ),
        (
            Forge::GitLab,
            RunChoice::default(),
            vec![refused()],
            gitlab::RUNS,
        ),
        (
            Forge::Forgejo,
            RunChoice::default(),
            vec![refused()],
            forgejo::RUNS,
        ),
    ];
    for (forge, choice, answers, what) in cases {
        let http = ScriptedHttp::new(answers);
        match asking_polled(forge, &choice, (&http, None)) {
            Err(Unpolled::Unreadable(reason)) => assert!(reason.contains(what), "{reason}"),
            other => panic!("{forge:?} {what}: {other:?}"),
        }
    }
}
