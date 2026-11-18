use std::collections::BTreeMap;

use herald_core::{Attribute, Identity, Validators};
use herald_ports::Response;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::options::Runs as RunsFile;
use crate::{RunFrom, SourceOptions};

const PAGE: &str = "https://github.com/o/r";

fn answer(status: u16, body: &Value, etag: Option<&str>) -> Response {
    Response {
        status,
        headers: etag
            .map(|etag| vec![("ETag".to_owned(), etag.to_owned())])
            .unwrap_or_default(),
        body: body.to_string().into_bytes(),
    }
}

fn repository() -> Repository {
    Repository {
        id: 7,
        name: "r".to_owned(),
        page: Some(PAGE.to_owned()),
    }
}

fn named() -> Value {
    json!({"id": 7, "name": "r", "full_name": "o/r", "html_url": PAGE, "visibility": "public"})
}

fn workflow_run(id: u64, status: &str, conclusion: Option<&str>, attempt: u64) -> Value {
    json!({
        "id": id,
        "name": "CI",
        "run_number": 41,
        "run_attempt": attempt,
        "event": "push",
        "status": status,
        "conclusion": conclusion,
        "head_branch": "main",
        "head_sha": "abc123",
        "html_url": format!("{PAGE}/actions/runs/{id}"),
        "display_title": "fix: the thing",
        "updated_at": "2026-11-17T10:00:00Z",
        "actor": {"login": "wessel"}
    })
}

fn choice(runs: RunsFile) -> RunChoice {
    SourceOptions {
        runs: Some(runs),
        ..SourceOptions::default()
    }
    .run_choice()
}

fn every_action() -> RunChoice {
    choice(RunsFile {
        from: Some(vec![RunFrom::Actions, RunFrom::Checks]),
        actions: Some(vec![
            RunAction::Requested,
            RunAction::InProgress,
            RunAction::Completed,
        ]),
        ..RunsFile::default()
    })
}

fn text(event: &Event, name: &str) -> Option<String> {
    match event.attributes().get(name) {
        Some(Attribute::Text(text)) => Some(text.clone()),
        Some(Attribute::Number(number)) => Some(number.to_string()),
        _ => None,
    }
}

fn delivered(forge: Forge, delivery: Delivery, payload: &Value) -> Option<Event> {
    delivered_event(
        forge,
        (delivery, payload.to_string().as_bytes()),
        &every_action(),
    )
    .unwrap()
    .1
}

#[test]
fn a_run_s_identity_names_its_attempt_or_how_it_ended_and_what_it_did_unfinished() {
    let run = |attempt: Option<u64>, action, conclusion| ForgeRun {
        ran: Ran::Run,
        id: 9,
        attempt,
        number: None,
        workflow: "CI".to_owned(),
        action,
        conclusion,
        trigger: None,
        branch: None,
        commit: None,
        actor: None,
        url: format!("{PAGE}/actions/runs/9"),
        title: None,
        occurred: jiff::Timestamp::UNIX_EPOCH,
    };
    let cases = [
        (
            Some(2),
            RunAction::Completed,
            Some(Conclusion::Success),
            "2",
            "CI succeeded",
        ),
        (
            Some(3),
            RunAction::InProgress,
            None,
            "3/in_progress",
            "CI started",
        ),
        (
            None,
            RunAction::Completed,
            Some(Conclusion::Failure),
            "failure",
            "CI failed",
        ),
        (None, RunAction::Completed, None, "completed", "CI finished"),
        (
            None,
            RunAction::Requested,
            None,
            "requested",
            "CI was requested",
        ),
    ];
    for (attempt, action, conclusion, last, title) in cases {
        let event = run(attempt, action, conclusion)
            .event("runs", &repository())
            .unwrap();
        assert_eq!(
            event.identity().as_str(),
            format!("github.com/7/run/9/{last}")
        );
        assert_eq!(event.title(), title);
        assert_eq!(event.body(), "");
    }
    let unlinked = ForgeRun {
        url: "not a page".to_owned(),
        ..run(None, RunAction::Completed, None)
    };
    let homeless = Repository {
        page: None,
        ..repository()
    };
    assert_eq!(
        unlinked.clone().event("runs", &homeless).unwrap_err(),
        SourceError::Unreadable {
            what: "runs",
            reason: "the repository's page names no host".to_owned()
        }
    );
    assert!(matches!(
        unlinked.event("runs", &repository()),
        Err(SourceError::Invalid { .. })
    ));
    let own = run(None, RunAction::Completed, None)
        .event("runs", &homeless)
        .unwrap();
    assert_eq!(own.identity().as_str(), "github.com/7/run/9/completed");
}

#[test]
fn every_conclusion_is_named_in_github_s_words_and_titled() {
    let titled: Vec<(&str, &str)> = Conclusion::ALL
        .into_iter()
        .map(|conclusion| {
            assert_eq!(Conclusion::named(conclusion.name()), Some(conclusion));
            (
                conclusion.name(),
                did(RunAction::Completed, Some(conclusion)),
            )
        })
        .collect();
    assert_eq!(
        titled,
        [
            ("success", "succeeded"),
            ("failure", "failed"),
            ("cancelled", "was cancelled"),
            ("timed_out", "timed out"),
            ("skipped", "was skipped"),
            ("action_required", "needs action"),
            ("neutral", "finished"),
            ("startup_failure", "failed to start"),
            ("stale", "went stale"),
        ]
    );
    assert_eq!(Conclusion::named("failed"), None);
    for (status, action) in [
        ("completed", RunAction::Completed),
        ("in_progress", RunAction::InProgress),
        ("queued", RunAction::Requested),
        ("waiting", RunAction::Requested),
    ] {
        assert_eq!(github_action(status), action, "{status}");
    }
}

#[test]
fn each_forge_names_the_events_its_runs_are_delivered_as() {
    let cases = [
        (
            Forge::GitHub,
            ("X-GitHub-Event", "workflow_run"),
            Some(Delivery::Run),
        ),
        (
            Forge::GitHub,
            ("x-github-event", "check_suite"),
            Some(Delivery::CheckSuite),
        ),
        (
            Forge::GitHub,
            ("X-GitHub-Event", "status"),
            Some(Delivery::Status),
        ),
        (Forge::GitHub, ("X-GitHub-Event", "release"), None),
        (
            Forge::GitLab,
            ("X-Gitlab-Event", "Pipeline Hook"),
            Some(Delivery::Run),
        ),
        (Forge::GitLab, ("X-Gitlab-Event", "Job Hook"), None),
        (
            Forge::Forgejo,
            ("X-Forgejo-Event", "workflow_run_failure"),
            Some(Delivery::Run),
        ),
        (
            Forge::Forgejo,
            ("X-Gitea-Event", "action_run_success"),
            Some(Delivery::Run),
        ),
        (Forge::Forgejo, ("X-Forgejo-Event", "workflow_run_"), None),
        (
            Forge::Forgejo,
            ("X-Forgejo-Event", "workflow_job_failure"),
            None,
        ),
    ];
    for (forge, header, delivery) in cases {
        assert_eq!(
            forge.run_delivery(&[("Content-Type", "x"), header]),
            delivery,
            "{header:?}"
        );
        assert_eq!(of_a_run(forge, header.1), delivery.is_some(), "{header:?}");
    }
    assert_eq!(Forge::GitHub.run_delivery(&[]), None);
    assert_eq!(
        Forge::GitHub.run_delivery(&[("X-Gitea-Event", "workflow_run")]),
        None
    );
    assert_eq!(
        [Forge::GitHub, Forge::GitLab, Forge::Forgejo].map(Forge::runs_what),
        [github::RUN, gitlab::RUN, forgejo::RUN]
    );
}

/// A failed run of the release workflow on main, started on a schedule.
fn finished() -> ForgeRun {
    ForgeRun {
        ran: Ran::Run,
        id: 1,
        attempt: Some(1),
        number: None,
        workflow: "Release".to_owned(),
        action: RunAction::Completed,
        conclusion: Some(Conclusion::Failure),
        trigger: Some("Schedule".to_owned()),
        branch: Some("main".to_owned()),
        commit: None,
        actor: None,
        url: PAGE.to_owned(),
        title: None,
        occurred: jiff::Timestamp::UNIX_EPOCH,
    }
}

#[test]
fn a_choice_takes_a_run_by_every_key_it_narrows_by() {
    let run = finished();
    assert!(run.chosen(&RunChoice::default()));
    let cases = [
        (
            RunsFile {
                conclusions: vec![Conclusion::Failure, Conclusion::TimedOut],
                ..RunsFile::default()
            },
            true,
        ),
        (
            RunsFile {
                conclusions: vec![Conclusion::Success],
                ..RunsFile::default()
            },
            false,
        ),
        (
            RunsFile {
                branches: vec!["ma*".to_owned()],
                ..RunsFile::default()
            },
            true,
        ),
        (
            RunsFile {
                branches: vec!["dev".to_owned()],
                ..RunsFile::default()
            },
            false,
        ),
        (
            RunsFile {
                workflows: vec!["rel*".to_owned()],
                ..RunsFile::default()
            },
            true,
        ),
        (
            RunsFile {
                workflows: vec!["ci".to_owned()],
                ..RunsFile::default()
            },
            false,
        ),
        (
            RunsFile {
                triggers: vec!["schedule".to_owned()],
                ..RunsFile::default()
            },
            true,
        ),
        (
            RunsFile {
                triggers: vec!["push".to_owned()],
                ..RunsFile::default()
            },
            false,
        ),
        (
            RunsFile {
                actions: Some(vec![RunAction::InProgress]),
                ..RunsFile::default()
            },
            false,
        ),
        (
            RunsFile {
                from: Some(vec![RunFrom::Checks]),
                ..RunsFile::default()
            },
            false,
        ),
    ];
    for (runs, takes) in cases {
        assert_eq!(run.chosen(&choice(runs.clone())), takes, "{runs:?}");
    }
}

#[test]
fn a_choice_narrowing_by_what_a_run_does_not_name_leaves_it_out() {
    let run = ForgeRun {
        conclusion: None,
        trigger: None,
        branch: None,
        ..finished()
    };
    for runs in [
        RunsFile {
            conclusions: vec![Conclusion::Failure],
            ..RunsFile::default()
        },
        RunsFile {
            branches: vec!["*".to_owned()],
            ..RunsFile::default()
        },
        RunsFile {
            triggers: vec!["push".to_owned()],
            ..RunsFile::default()
        },
    ] {
        assert!(!run.chosen(&choice(runs.clone())), "{runs:?}");
    }
}

#[test]
fn a_ci_run_s_own_event_announces_its_run_from_its_repository() {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    let payload = json!({"workflow_run": workflow_run(9, "completed", Some("success"), 1), "repository": named()});
    let event = ci_run(
        (Forge::GitHub, &options),
        ("workflow_run", payload.to_string().as_bytes()),
    )
    .unwrap()
    .unwrap();
    let origin = event.origin().unwrap();
    assert_eq!(
        (origin.repository.as_str(), origin.host.as_str()),
        ("o/r", "github.com")
    );
    assert_eq!(
        ci_run((Forge::GitHub, &options), ("release", b"{}")).unwrap(),
        None
    );
    let unfinished =
        json!({"workflow_run": workflow_run(9, "in_progress", None, 1), "repository": named()});
    assert_eq!(
        ci_run(
            (Forge::GitHub, &options),
            ("workflow_run", unfinished.to_string().as_bytes())
        )
        .unwrap(),
        None
    );
    assert!(ci_run((Forge::GitHub, &options), ("workflow_run", b"{}")).is_err());
}

mod forges;
mod polls;
