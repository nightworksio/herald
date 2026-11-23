use herald_core::{BaseKey, SourceName, Validators};
use herald_ports::Response;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::SourceOptions;

fn answer(status: u16, body: &Value) -> Response {
    tagged(status, body, None)
}

fn tagged(status: u16, body: &Value, etag: Option<&str>) -> Response {
    Response {
        status,
        headers: etag
            .map(|etag| ("etag".to_owned(), etag.to_owned()))
            .into_iter()
            .collect(),
        body: body.to_string().into_bytes(),
    }
}

fn read(
    forge: Forge,
    http: &ScriptedHttp,
    (branch, kept): (&str, Option<&KeptBase>),
) -> Result<BaseFound, Unpolled> {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    let origin = Origin::new(forge, &options, "o/r");
    let http = Erased(http as &dyn DynHttp);
    pollster::block_on(based(&http, (&origin, None), branch, kept))
}

fn kept(parts: Vec<KeptCheck>) -> KeptBase {
    let key = BaseKey {
        source: SourceName::new("s"),
        repository: "o/r".to_owned(),
        branch: "main".to_owned(),
    };
    KeptBase::of(key, parts, jiff::Timestamp::UNIX_EPOCH).unwrap()
}

fn part(etag: &str, checks: Checks) -> KeptCheck {
    KeptCheck {
        validators: Validators {
            etag: Some(etag.to_owned()),
            last_modified: None,
        },
        checks,
    }
}

fn failed(names: &[&str]) -> Checks {
    Checks::Failed(names.iter().map(|name| (*name).to_owned()).collect())
}

fn runs(runs: &[(&str, &str, Option<&str>)]) -> Value {
    let runs: Vec<Value> = runs
        .iter()
        .map(|(name, status, conclusion)| {
            json!({ "name": name, "status": status, "conclusion": conclusion })
        })
        .collect();
    json!({ "total_count": runs.len(), "check_runs": runs })
}

#[test]
fn a_github_base_branch_joins_its_check_runs_and_its_commit_statuses() {
    let http = ScriptedHttp::new(vec![
        tagged(
            200,
            &runs(&[
                ("build", "completed", Some("success")),
                ("test", "completed", Some("failure")),
            ]),
            Some("\"runs\""),
        ),
        tagged(
            200,
            &json!({ "state": "failure", "statuses": [
                { "context": "ci/legacy", "state": "error" },
                { "context": "ci/ok", "state": "success" },
            ] }),
            Some("\"status\""),
        ),
    ]);
    let found = read(Forge::GitHub, &http, ("release/v2", None)).unwrap();
    assert_eq!(
        http.asked(),
        [
            (
                "https://api.github.com/repos/o/r/commits/release%2Fv2/check-runs?per_page=100"
                    .to_owned(),
                None
            ),
            (
                "https://api.github.com/repos/o/r/commits/release%2Fv2/status?per_page=100"
                    .to_owned(),
                None
            ),
        ]
    );
    assert_eq!(
        found.read,
        BaseRead::Checked(failed(&["test", "ci/legacy"]))
    );
    assert_eq!(
        found.parts,
        [
            part("\"runs\"", failed(&["test"])),
            part("\"status\"", failed(&["ci/legacy"])),
        ]
    );
    let pending = ScriptedHttp::new(vec![
        answer(200, &runs(&[("build", "in_progress", None)])),
        answer(
            200,
            &json!({ "state": "pending", "statuses": [{ "context": "c", "state": "pending" }] }),
        ),
    ]);
    let found = read(Forge::GitHub, &pending, ("main", None)).unwrap();
    assert_eq!(found.read, BaseRead::Checked(Checks::Pending));
    let none = ScriptedHttp::new(vec![
        answer(200, &runs(&[])),
        answer(200, &json!({ "state": "pending", "statuses": [] })),
    ]);
    let found = read(Forge::GitHub, &none, ("main", None)).unwrap();
    assert_eq!(found.read, BaseRead::Checked(Checks::Absent));
}

#[test]
fn an_unchanged_answer_is_read_as_kept_and_a_changed_one_again() {
    let before = kept(vec![
        part("\"runs\"", failed(&["test"])),
        part("\"status\"", Checks::Passed),
    ]);
    let http = ScriptedHttp::new(vec![
        tagged(304, &json!(null), Some("\"runs\"")),
        tagged(
            200,
            &json!({ "statuses": [{ "context": "c", "state": "failure" }] }),
            Some("\"new\""),
        ),
    ]);
    let found = read(Forge::GitHub, &http, ("main", Some(&before))).unwrap();
    let etags: Vec<Option<String>> = http.asked().into_iter().map(|(_, etag)| etag).collect();
    assert_eq!(
        etags,
        [Some("\"runs\"".to_owned()), Some("\"status\"".to_owned())]
    );
    assert_eq!(found.read, BaseRead::Checked(failed(&["test", "c"])));
    assert_eq!(
        found.parts,
        [
            part("\"runs\"", failed(&["test"])),
            part("\"new\"", failed(&["c"]))
        ]
    );
    let unfitting = kept(vec![part("\"one\"", Checks::Passed)]);
    let http = ScriptedHttp::new(vec![
        answer(200, &runs(&[("build", "completed", Some("success"))])),
        answer(200, &json!({ "statuses": [] })),
    ]);
    let found = read(Forge::GitHub, &http, ("main", Some(&unfitting))).unwrap();
    assert!(http.asked().iter().all(|(_, etag)| etag.is_none()));
    assert_eq!(found.read, BaseRead::Checked(Checks::Passed));
    assert!(
        found
            .parts
            .iter()
            .all(|part| part.validators == Validators::default())
    );
}

#[test]
fn a_refused_base_branch_is_named_unread_with_the_permission_it_needs() {
    let cases = [
        (Forge::GitHub, 403, GITHUB_REFUSED),
        (Forge::GitHub, 401, GITHUB_REFUSED),
        (Forge::GitLab, 403, GITLAB_REFUSED),
        (Forge::GitLab, 401, GITLAB_REFUSED),
        (Forge::Forgejo, 403, FORGEJO_REFUSED),
        (Forge::Forgejo, 401, FORGEJO_REFUSED),
        (Forge::GitHub, 404, UNSEEN),
        (Forge::GitLab, 404, UNSEEN),
        (Forge::Forgejo, 404, UNSEEN),
    ];
    for (forge, status, reason) in cases {
        let http = ScriptedHttp::new(vec![answer(status, &json!({ "message": "no" }))]);
        let found = read(forge, &http, ("main", None)).unwrap();
        assert_eq!(
            found.read,
            BaseRead::Unread(reason.to_owned()),
            "{forge:?} {status}"
        );
        assert_eq!(found.parts, []);
        assert_eq!(http.asked().len(), 1);
    }
    let later = ScriptedHttp::new(vec![
        answer(200, &runs(&[])),
        answer(403, &json!({ "message": "no" })),
    ]);
    let found = read(Forge::GitHub, &later, ("main", None)).unwrap();
    assert_eq!(found.read, BaseRead::Unread(GITHUB_REFUSED.to_owned()));
}

#[test]
fn a_rate_limit_or_a_failed_answer_stops_the_read() {
    for forge in [Forge::GitHub, Forge::GitLab, Forge::Forgejo] {
        let http = ScriptedHttp::new(vec![answer(429, &json!({}))]);
        let stopped = read(forge, &http, ("main", None)).unwrap_err();
        assert!(
            matches!(stopped, Unpolled::RateLimited(_)),
            "{forge:?} {stopped:?}"
        );
        let http = ScriptedHttp::new(vec![answer(500, &json!({}))]);
        let stopped = read(forge, &http, ("main", None)).unwrap_err();
        assert!(
            matches!(stopped, Unpolled::Unreadable(_)),
            "{forge:?} {stopped:?}"
        );
        let http = ScriptedHttp::new(vec![answer(200, &json!("no state"))]);
        let stopped = read(forge, &http, ("main", None)).unwrap_err();
        assert!(
            matches!(stopped, Unpolled::Unreadable(_)),
            "{forge:?} {stopped:?}"
        );
    }
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([{ "id": 9, "status": "failed" }])),
        answer(429, &json!({})),
    ]);
    let stopped = read(Forge::GitLab, &http, ("main", None)).unwrap_err();
    assert!(matches!(stopped, Unpolled::RateLimited(_)), "{stopped:?}");
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([{ "id": 9, "status": "failed" }])),
        answer(200, &json!({ "jobs": "none" })),
    ]);
    let stopped = read(Forge::GitLab, &http, ("main", None)).unwrap_err();
    assert!(matches!(stopped, Unpolled::Unreadable(_)), "{stopped:?}");
    let http = ScriptedHttp::new(vec![
        answer(200, &runs(&[])),
        answer(200, &json!("no status")),
    ]);
    let stopped = read(Forge::GitHub, &http, ("main", None)).unwrap_err();
    assert!(matches!(stopped, Unpolled::Unreadable(_)), "{stopped:?}");
}

#[test]
fn a_gitlab_base_branch_is_its_newest_pipeline_naming_the_jobs_that_failed() {
    let http = ScriptedHttp::new(vec![
        tagged(
            200,
            &json!([{ "id": 9, "status": "failed" }]),
            Some("\"p\""),
        ),
        answer(
            200,
            &json!([
                { "name": "test", "allow_failure": false },
                { "name": "flaky", "allow_failure": true },
                { "name": "lint" },
            ]),
        ),
    ]);
    let found = read(Forge::GitLab, &http, ("release/v2", None)).unwrap();
    assert_eq!(
        http.asked(),
        [
            (
                "https://gitlab.com/api/v4/projects/o%2Fr/pipelines?ref=release%2Fv2&per_page=1"
                    .to_owned(),
                None
            ),
            (
                "https://gitlab.com/api/v4/projects/o%2Fr/pipelines/9/jobs?scope[]=failed&per_page=100"
                    .to_owned(),
                None
            ),
        ]
    );
    assert_eq!(found.read, BaseRead::Checked(failed(&["test", "lint"])));
    assert_eq!(found.parts, [part("\"p\"", failed(&["test", "lint"]))]);
    let allowed = ScriptedHttp::new(vec![
        answer(200, &json!([{ "id": 9, "status": "canceled" }])),
        answer(200, &json!([{ "name": "flaky", "allow_failure": true }])),
    ]);
    let found = read(Forge::GitLab, &allowed, ("main", None)).unwrap();
    assert_eq!(found.read, BaseRead::Checked(failed(&[PIPELINE])));
    let states = [
        ("success", Checks::Passed),
        ("skipped", Checks::Passed),
        ("running", Checks::Pending),
        ("manual", Checks::Pending),
    ];
    for (status, checks) in states {
        let http = ScriptedHttp::new(vec![answer(200, &json!([{ "id": 9, "status": status }]))]);
        let found = read(Forge::GitLab, &http, ("main", None)).unwrap();
        assert_eq!(found.read, BaseRead::Checked(checks), "{status}");
        assert_eq!(http.asked().len(), 1);
    }
    let none = ScriptedHttp::new(vec![answer(200, &json!([]))]);
    let found = read(Forge::GitLab, &none, ("main", None)).unwrap();
    assert_eq!(found.read, BaseRead::Checked(Checks::Absent));
}

#[test]
fn a_forgejo_base_branch_is_its_combined_status() {
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!({ "state": "failure", "statuses": [
            { "context": "ci/build", "status": "failure" },
            { "context": "ci/lint", "status": "success" },
        ] }),
    )]);
    let found = read(Forge::Forgejo, &http, ("main", None)).unwrap();
    assert_eq!(found.read, BaseRead::Checked(failed(&["ci/build"])));
    assert_eq!(http.asked().len(), 1);
    assert!(
        http.asked()[0]
            .0
            .ends_with("/repos/o/r/commits/main/status?limit=100")
    );
}
