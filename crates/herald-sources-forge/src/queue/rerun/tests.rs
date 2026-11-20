use herald_kinds::Queue as _;
use herald_ports::{Response, Secret};
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::queue::Queued;
use crate::{Origin, SourceOptions, Token};

fn answer(body: &Value) -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn queued(forge: Forge) -> Queued {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    Queued {
        origin: Origin::new(forge, &options, "o/r"),
        token: Some(Token::Named(Secret::new("TOKEN", "t"))),
        project: "r".to_owned(),
        graphql: "https://api.github.com/graphql".to_owned(),
    }
}

fn planned(
    queue: &dyn herald_kinds::Queue,
    answers: Vec<Response>,
) -> (Result<Plan, Unpolled>, Vec<String>) {
    planned_most(queue, answers, herald_core::MOST_RUNS)
}

fn planned_most(
    queue: &dyn herald_kinds::Queue,
    answers: Vec<Response>,
    most: usize,
) -> (Result<Plan, Unpolled>, Vec<String>) {
    let http = ScriptedHttp::new(answers);
    let plan = pollster::block_on(queue.planned(&http, ("o/r", "h1", "test"), most));
    let asked = http.asked().into_iter().map(|(url, _)| url).collect();
    (plan, asked)
}

#[test]
fn github_plans_each_failed_actions_run_on_the_head_whose_job_of_the_check_failed() {
    let runs = json!({ "workflow_runs": [
        { "id": 42, "html_url": "https://github.com/o/r/actions/runs/42" },
        { "id": 43, "html_url": "https://github.com/o/r/actions/runs/43" }
    ] });
    let (plan, asked) = planned(
        &queued(Forge::GitHub),
        vec![
            answer(&runs),
            answer(
                &json!({ "jobs": [{ "id": 420, "name": "lint", "conclusion": "failure" }, { "id": 421, "name": "test", "conclusion": "failure" }] }),
            ),
            answer(
                &json!({ "jobs": [{ "id": 430, "name": "test", "conclusion": "success" }, { "id": 431, "name": "lint", "conclusion": "failure" }] }),
            ),
        ],
    );
    assert_eq!(
        plan.unwrap(),
        Plan::Runs(vec![Rerun {
            run: "42".to_owned(),
            job: "421".to_owned(),
            page: "https://github.com/o/r/actions/runs/42".to_owned(),
            call: "POST https://api.github.com/repos/o/r/actions/jobs/421/rerun".to_owned(),
        }])
    );
    assert_eq!(
        asked,
        [
            "https://api.github.com/repos/o/r/actions/runs?head_sha=h1&status=failure&per_page=5",
            "https://api.github.com/repos/o/r/actions/runs/42/jobs?filter=latest&per_page=100",
            "https://api.github.com/repos/o/r/actions/runs/43/jobs?filter=latest&per_page=100",
        ]
    );
    let (none, _) = planned(&queued(Forge::GitHub), vec![answer(&json!({}))]);
    assert_eq!(none.unwrap(), Plan::Unplanned(NO_RUN));
}

#[test]
fn gitlab_plans_each_failed_pipeline_whose_job_of_the_check_failed_and_forgejo_says_why_not() {
    let pipelines = json!([
        { "id": 7, "web_url": "https://gitlab.com/o/r/-/pipelines/7" },
        { "id": 6, "web_url": "https://gitlab.com/o/r/-/pipelines/6" }
    ]);
    let (plan, asked) = planned(
        &queued(Forge::GitLab),
        vec![
            answer(&pipelines),
            answer(&json!([{ "id": 70, "name": "lint" }, { "id": 71, "name": "test" }])),
            answer(&json!([{ "id": 60, "name": "lint" }])),
        ],
    );
    assert_eq!(
        plan.unwrap(),
        Plan::Runs(vec![Rerun {
            run: "7".to_owned(),
            job: "71".to_owned(),
            page: "https://gitlab.com/o/r/-/pipelines/7".to_owned(),
            call: "POST https://gitlab.com/api/v4/projects/o%2Fr/jobs/71/retry".to_owned(),
        }])
    );
    assert_eq!(
        asked,
        [
            "https://gitlab.com/api/v4/projects/o%2Fr/pipelines?sha=h1&status=failed&per_page=5",
            "https://gitlab.com/api/v4/projects/o%2Fr/pipelines/7/jobs?scope=failed&per_page=100",
            "https://gitlab.com/api/v4/projects/o%2Fr/pipelines/6/jobs?scope=failed&per_page=100",
        ]
    );
    let (none, _) = planned(&queued(Forge::GitLab), vec![answer(&json!([]))]);
    assert_eq!(none.unwrap(), Plan::Unplanned(NO_PIPELINE));
    let (forgejo, asked) = planned(&queued(Forge::Forgejo), Vec::new());
    assert_eq!(forgejo.unwrap(), Plan::Unplanned(UNRERUN));
    assert_eq!(asked.len(), 0);
}

#[test]
fn runs_that_cannot_be_read_fail_the_plan() {
    for (forge, body) in [(Forge::GitHub, json!("runs")), (Forge::GitLab, json!({}))] {
        let (failed, _) = planned(&queued(forge), vec![answer(&body)]);
        assert!(matches!(failed, Err(Unpolled::Unreadable(_))), "{forge:?}");
    }
    let runs = json!({ "workflow_runs": [{ "id": 42, "html_url": "https://github.com/o/r/actions/runs/42" }] });
    let (failed, _) = planned(
        &queued(Forge::GitHub),
        vec![answer(&runs), answer(&json!("jobs"))],
    );
    assert!(matches!(failed, Err(Unpolled::Unreadable(_))));
    let pipelines = json!([{ "id": 7, "web_url": "https://gitlab.com/o/r/-/pipelines/7" }]);
    let (failed, _) = planned(
        &queued(Forge::GitLab),
        vec![answer(&pipelines), answer(&json!({}))],
    );
    assert!(matches!(failed, Err(Unpolled::Unreadable(_))));
}

#[test]
fn a_queue_holds_its_own_repository_and_an_owner_s_the_repositories_it_holds() {
    let queue = queued(Forge::GitHub);
    assert!(queue.holds("O/R"));
    assert!(!queue.holds("o/other"));
    let options = SourceOptions {
        owner: Some("group".to_owned()),
        ..SourceOptions::default()
    };
    let owned = crate::owner::Owned::new(Forge::GitLab, ("group", None), &options, (None, 0));
    let scoped = crate::queue::Scoped::new(
        (Forge::GitLab, "group"),
        &options,
        None,
        ("https://gitlab.com/api/graphql".to_owned(), owned),
    );
    assert!(scoped.holds("group/sub/app"));
    assert!(scoped.holds("Group/app"));
    assert!(!scoped.holds("other/app"));
    assert!(!scoped.holds("app"));
    let (plan, asked) = planned(&scoped, vec![answer(&json!([]))]);
    assert_eq!(plan.unwrap(), Plan::Unplanned(NO_PIPELINE));
    assert_eq!(
        asked,
        ["https://gitlab.com/api/v4/projects/o%2Fr/pipelines?sha=h1&status=failed&per_page=5"]
    );
}

#[test]
fn a_plan_reads_only_the_newest_runs_it_is_bounded_to() {
    let runs = json!({ "workflow_runs": [
        { "id": 42, "html_url": "https://github.com/o/r/actions/runs/42" },
        { "id": 41, "html_url": "https://github.com/o/r/actions/runs/41" }
    ] });
    let failed = json!({ "jobs": [{ "id": 420, "name": "test", "conclusion": "failure" }] });
    let (plan, asked) = planned_most(
        &queued(Forge::GitHub),
        vec![answer(&runs), answer(&failed)],
        1,
    );
    assert!(matches!(plan.unwrap(), Plan::Runs(runs) if runs.len() == 1));
    assert_eq!(
        asked,
        [
            "https://api.github.com/repos/o/r/actions/runs?head_sha=h1&status=failure&per_page=1",
            "https://api.github.com/repos/o/r/actions/runs/42/jobs?filter=latest&per_page=100",
        ]
    );
    let pipelines = json!([
        { "id": 8, "web_url": "https://gitlab.com/o/r/-/pipelines/8" },
        { "id": 7, "web_url": "https://gitlab.com/o/r/-/pipelines/7" }
    ]);
    let (plan, _) = planned_most(
        &queued(Forge::GitLab),
        vec![
            answer(&pipelines),
            answer(&json!([{ "id": 80, "name": "test" }])),
        ],
        1,
    );
    assert!(matches!(plan.unwrap(), Plan::Runs(runs) if runs.len() == 1 && runs[0].run == "8"));
}

#[test]
fn a_repository_whose_path_holds_a_dot_segment_plans_nothing_and_asks_nothing() {
    for forge in [Forge::GitHub, Forge::GitLab] {
        let options = SourceOptions {
            owner: Some("o".to_owned()),
            ..SourceOptions::default()
        };
        let owned = crate::owner::Owned::new(forge, ("o", None), &options, (None, 0));
        let scoped = crate::queue::Scoped::new(
            (forge, "o"),
            &options,
            None,
            ("https://api.github.com/graphql".to_owned(), owned),
        );
        let http = ScriptedHttp::new(Vec::new());
        let plan = pollster::block_on(scoped.planned(&http, ("o/..", "h1", "test"), 5));
        assert_eq!(
            plan,
            Err(Unpolled::Unreadable(
                "the repository's path \"..\" is empty or a dot segment, or holds one, which names nothing in a URL path, so it is not asked for"
                    .to_owned()
            )),
            "{forge:?}"
        );
        assert_eq!(http.asked().len(), 0, "{forge:?}");
    }
}
