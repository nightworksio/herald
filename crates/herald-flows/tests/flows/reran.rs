//! `herald rerun`: a rule's plans listed without the write credential, and
//! a job applied once by a signed-in operator, after the checks made right
//! before the call, with the write credential alone (ADR-0068).

use std::sync::Mutex;

use herald_config::{Config, Uses, load};
use herald_core::{Reran, RunKey};
use herald_flows::{
    Applied, Applying, Budget, Connected, Done, Ports, Requests, Stop, UNCHOSEN, UNKEPT, UNPLANNED,
    prepare, rerun_apply, rerun_plans,
};
use herald_ports::{Actions as _, Base};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeTimer};
use serde_json::json;

use crate::boards::flaky;
use crate::harness::{Failing, Fault, NOON, Script, healthy};

const TOKEN_WRITE: &str = r#"write = { token = { secret = "HERALD_RERUN" } }
sign_in = { client_id = "Iv23liClient" }"#;

const RUNS: &str = "/repos/o/open/actions/runs?head_sha=h1&status=failure&per_page=5";
const JOBS: &str = "/repos/o/open/actions/runs/42/jobs?filter=latest&per_page=100";
const JOB: &str = "/repos/o/open/actions/jobs/420";
const DEVICE: &str = "https://github.com/login/device/code";
const SIGNED: &str = "https://github.com/login/oauth/access_token";
const USER: &str = "https://api.github.com/user";
const INSTALLATIONS: &str = "https://api.github.com/user/installations?per_page=100&page=1";
const SIGN_IN_APP: &str = "https://api.github.com/apps/herald-sign-in";
const REPOSITORY: &str = "https://api.github.com/repos/o/open";
const PULL: &str = "/repos/o/open/pulls/1";
const RUN: &str = "/repos/o/open/actions/runs/42";
const CALL: &str = "/repos/o/open/actions/jobs/420/rerun";

const TOKEN_READ: &str = r#"token = { secret = "HERALD_GITHUB_TOKEN" }"#;

fn config(write: &str) -> Config {
    config_reading(TOKEN_READ, write)
}

fn config_reading(read: &str, write: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.open]
kind = "github"
repository = "o/open"
{read}
{write}
[views.kept]
sources = ["open"]
mode = "kept"
reconcile = "1d"
[rules.flaky]
view = "kept"
flaky = {{ rerun = true }}
every = "1h"
at = "00:00"
[github_apps.rerun]
client_id = "Iv23liRerun"
private_key = {{ secret = "HERALD_RERUN_KEY" }}
[github_apps.reader]
client_id = "Iv23liReader"
private_key = {{ secret = "HERALD_READER_KEY" }}
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn secrets() -> FakeSecrets {
    FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "discord-value")
        .with("HERALD_GITHUB_TOKEN", "read-value")
        .with("HERALD_RERUN", "write-value")
        .with("HERALD_RERUN_KEY", "the App's key")
        .with("HERALD_READER_KEY", "the reading App's key")
}

/// The plan's answers: the failed runs on `h1`, and run 42's jobs, whose
/// `test`, job 420, failed.
fn planned() -> Vec<(&'static str, u16, String)> {
    let runs = json!({ "workflow_runs": [
        { "id": 42, "html_url": "https://github.com/o/open/actions/runs/42" }
    ] });
    let jobs = json!({ "jobs": [{ "id": 420, "name": "test", "conclusion": "failure" }] });
    vec![(RUNS, 200, runs.to_string()), (JOBS, 200, jobs.to_string())]
}

/// The operator's token: a GitHub App's user token.
fn operator() -> String {
    format!(
        "{}operator-value",
        herald_sources_forge::GITHUB_APP_USER_TOKEN
    )
}

/// The sign-in's answers: a device code, one wait, then the token, the
/// operator it names, the one installation of the sign-in's App it
/// reaches, and that App, each of which only reads.
fn signed_in() -> Vec<(&'static str, u16, String)> {
    vec![
        (
            DEVICE,
            200,
            json!({ "device_code": "device-value", "user_code": "WDJB-MJHT",
                "verification_uri": "https://github.com/login/device" })
            .to_string(),
        ),
        (
            SIGNED,
            200,
            json!({ "error": "authorization_pending" }).to_string(),
        ),
        (
            SIGNED,
            200,
            json!({ "access_token": operator() }).to_string(),
        ),
        (USER, 200, json!({ "login": "octocat" }).to_string()),
        (
            INSTALLATIONS,
            200,
            json!({ "installations": [{ "app_id": 1, "app_slug": "herald-sign-in",
                "client_id": "Iv23liClient",
                "permissions": { "metadata": "read", "pull_requests": "read" } }] })
            .to_string(),
        ),
        (
            SIGN_IN_APP,
            200,
            json!({ "id": 1, "slug": "herald-sign-in",
                "permissions": { "metadata": "read", "pull_requests": "read", "emails": "read" } })
            .to_string(),
        ),
    ]
}

/// The checks right before the call: the operator may push, the pull
/// request's head is `head`, and job 420 of run 42 ran on `h1` and failed,
/// in the run's attempt `attempt`.
fn checked(head: &str, attempt: u64) -> Vec<(&'static str, u16, String)> {
    let mut checks = vec![
        (
            REPOSITORY,
            200,
            json!({ "permissions": { "push": true } }).to_string(),
        ),
        (
            PULL,
            200,
            json!({ "state": "open", "head": { "sha": head } }).to_string(),
        ),
    ];
    checks.extend(job_read(attempt, "failure"));
    checks
}

/// Job 420 read: ended as `conclusion` on `h1`, in run 42's attempt
/// `attempt`.
fn job_read(attempt: u64, conclusion: &str) -> Vec<(&'static str, u16, String)> {
    vec![
        (JOB, 200, job(42, "h1", conclusion)),
        (RUN, 200, json!({ "run_attempt": attempt }).to_string()),
    ]
}

fn job(run: u64, head: &str, conclusion: &str) -> String {
    json!({ "run_id": run, "head_sha": head, "status": "completed", "conclusion": conclusion })
        .to_string()
}

fn scripted(parts: &[Vec<(&'static str, u16, String)>]) -> Script {
    let answers: Vec<(&str, u16, &str)> = parts
        .iter()
        .flatten()
        .map(|(path, status, body)| (*path, *status, body.as_str()))
        .collect();
    Script::answering(&answers)
}

/// What one apply did.
struct Ended {
    applied: Result<Vec<Applied>, Stop>,
    bases: Vec<String>,
    told: Vec<String>,
    waited: usize,
}

/// `chosen` applied under `config` over `store`, every connection answered
/// by `script`, within `most` requests where it says.
fn apply(
    given: (&Config, &Failing, &FakeSecrets),
    script: &Script,
    asked: (&[(&str, &str)], Option<usize>),
) -> Ended {
    applied_by(given, script, asked, None)
}

/// [`apply`], every connection to `refused` refused.
fn applied_by(
    given: (&Config, &Failing, &FakeSecrets),
    script: &Script,
    asked: (&[(&str, &str)], Option<usize>),
    refused: Option<&str>,
) -> Ended {
    applied_from(given.0, given, script, asked, refused)
}

/// [`applied_by`], its sources prepared from `prepared_from`.
fn applied_from(
    prepared_from: &Config,
    (config, store, secrets): (&Config, &Failing, &FakeSecrets),
    script: &Script,
    (chosen, most): (&[(&str, &str)], Option<usize>),
    refused: Option<&str>,
) -> Ended {
    let prepared = pollster::block_on(prepare(
        prepared_from,
        &self::secrets(),
        &FakeSigner,
        Uses::Polls,
    ))
    .unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget {
            requests: most.map(|most| Requests {
                made: script.made(),
                from: script.made().load(std::sync::atomic::Ordering::Relaxed),
                most,
                spare: 0,
            }),
            ..Budget::UNBOUNDED
        },
    };
    let bases = Mutex::new(Vec::new());
    let told = Mutex::new(Vec::new());
    let connect = |base: Base<'_>| {
        let base = base.to_string();
        if refused == Some(base.as_str()) {
            return Err(format!("{base} cannot be reached"));
        }
        bases.lock().unwrap().push(base);
        Ok(script.clone())
    };
    let tell = |signing: herald_flows::SigningIn<'_>| {
        told.lock().unwrap().push(format!(
            "{} {} {}",
            signing.source, signing.verification, signing.user_code
        ));
    };
    let timer = FakeTimer::default();
    let applying = Applying {
        secrets,
        connect: &connect,
        timer: &timer,
        told: &tell,
    };
    let chosen: Vec<(String, String)> = chosen
        .iter()
        .map(|(job, head)| ((*job).to_owned(), (*head).to_owned()))
        .collect();
    let applied = pollster::block_on(rerun_apply(
        (config, &connected, &ports),
        &applying,
        (Some("flaky"), &chosen),
    ));
    Ended {
        applied,
        bases: bases.into_inner().unwrap(),
        told: told.into_inner().unwrap(),
        waited: timer.waited().len(),
    }
}

fn done(applied: &Ended) -> Vec<Done> {
    applied
        .applied
        .as_ref()
        .unwrap()
        .iter()
        .map(|applied| applied.done.clone())
        .collect()
}

fn refused(applied: &Ended) -> String {
    match done(applied).as_slice() {
        [Done::Refused(reason)] => reason.clone(),
        other => panic!("{other:?}"),
    }
}

fn key() -> RunKey {
    RunKey {
        api: "https://api.github.com".to_owned(),
        repository: "o/open".to_owned(),
        run: "42".to_owned(),
        job: "420".to_owned(),
        head: "h1".to_owned(),
    }
}

/// Every request carrying `value` as its bearer token, by its line.
fn carrying(script: &Script, value: &str) -> Vec<String> {
    let bearer = format!("Bearer {value}");
    script
        .seen()
        .into_iter()
        .zip(script.asked())
        .filter(|(_, headers)| headers.iter().any(|(_, header)| *header == bearer))
        .map(|(line, _)| line)
        .collect()
}

/// What a script answers, in order: each path, its status and its body.
type Answers = Vec<(&'static str, u16, String)>;

const CHOSEN: &[(&str, &str)] = &[("420", "h1")];

#[test]
fn a_plan_names_each_job_and_its_call_without_reading_the_write_credential() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let script = scripted(&[planned()]);
    let secrets = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "discord-value")
        .with("HERALD_GITHUB_TOKEN", "read-value");
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let planned = pollster::block_on(rerun_plans((&config, &connected, &ports), None)).unwrap();
    assert_eq!(planned.len(), 1);
    let plan = &planned[0];
    assert_eq!(
        (
            plan.rule.as_str(),
            plan.repository.as_str(),
            plan.number,
            plan.head.as_str()
        ),
        ("flaky", "o/open", 1, "h1")
    );
    assert_eq!(
        plan.run.as_ref().unwrap().call,
        "POST https://api.github.com/repos/o/open/actions/jobs/420/rerun"
    );
    assert_eq!(carrying(&script, "write-value"), Vec::<String>::new());
    let unknown = pollster::block_on(rerun_plans((&config, &connected, &ports), Some("nope")));
    assert!(
        matches!(unknown, Err(Stop::Source(ref reason)) if reason.contains("no rule is named"))
    );
}

#[test]
fn a_signed_in_operator_re_runs_a_planned_job_once_with_the_write_credential_alone() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let script = scripted(&[
        planned(),
        signed_in(),
        checked("h1", 1),
        vec![(CALL, 201, String::new())],
    ]);
    let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
    assert_eq!(done(&applied), [Done::Rerun]);
    assert_eq!(
        applied.bases,
        ["https://github.com", "https://api.github.com"]
    );
    assert_eq!(
        applied.told,
        ["open https://github.com/login/device WDJB-MJHT"]
    );
    assert_eq!(applied.waited, 2);
    assert_eq!(
        carrying(&script, "write-value"),
        [format!("POST https://api.github.com{CALL}")]
    );
    assert_eq!(
        carrying(&script, &operator()),
        [
            format!("GET {USER}"),
            format!("GET {INSTALLATIONS}"),
            format!("GET {SIGN_IN_APP}"),
            format!("GET {REPOSITORY}")
        ]
    );
    let read = carrying(&script, "read-value");
    assert!(read.iter().all(|line| line.starts_with("GET ")), "{read:?}");
    let device = script.seen();
    assert!(
        !script.asked()[4]
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization")),
        "{device:?}"
    );
    let record = pollster::block_on(store.inner.action(&key()))
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            record.value.operator.as_str(),
            record.value.credential.as_str(),
            record.value.outcome,
            record.value.at,
        ),
        ("octocat", "HERALD_RERUN", Reran::Rerun, NOON)
    );
    let again = script_with_sign_in(checked("h1", 1));
    let applied = apply((&config, &store, &secrets()), &again, (CHOSEN, None));
    assert!(
        refused(&applied).contains("already asked"),
        "{:?}",
        applied.applied
    );
    assert_eq!(carrying(&again, "write-value"), Vec::<String>::new());
}

fn script_with_sign_in(checks: Vec<(&'static str, u16, String)>) -> Script {
    scripted(&[planned(), signed_in(), checks])
}

#[test]
fn each_check_right_before_the_call_refuses_without_a_write() {
    let config = config(TOKEN_WRITE);
    let cases: Vec<(Answers, &str)> = vec![
        (checked("h2", 1), "head moved to h2"),
        (checked("h1", 2), "already re-run 1 times"),
        (
            [checked("h1", 1)[..2].to_vec(), job_read(1, "success")].concat(),
            "no longer reads as failed",
        ),
        (
            vec![(
                REPOSITORY,
                200,
                json!({ "permissions": { "pull": true } }).to_string(),
            )],
            "octocat holds read or none on o/open",
        ),
        (
            vec![(REPOSITORY, 500, String::new())],
            "role on o/open could not be read",
        ),
        (
            vec![
                checked("h1", 1)[0].clone(),
                (
                    PULL,
                    200,
                    json!({ "state": "closed", "head": { "sha": "h1" } }).to_string(),
                ),
            ],
            "no longer open",
        ),
        (
            vec![checked("h1", 1)[0].clone(), (PULL, 500, String::new())],
            "pull request's head",
        ),
        (
            [
                checked("h1", 1)[..2].to_vec(),
                vec![(JOB, 500, String::new())],
            ]
            .concat(),
            "a job",
        ),
        (
            [
                checked("h1", 1)[..3].to_vec(),
                vec![(RUN, 500, String::new())],
            ]
            .concat(),
            "a run",
        ),
        (
            [
                checked("h1", 1)[..2].to_vec(),
                vec![
                    (JOB, 200, job(42, "h9", "failure")),
                    (RUN, 200, json!({ "run_attempt": 1 }).to_string()),
                ],
            ]
            .concat(),
            "ran on h9",
        ),
        (
            [
                checked("h1", 1)[..2].to_vec(),
                vec![(JOB, 200, job(41, "h1", "failure"))],
            ]
            .concat(),
            "not of the run 42",
        ),
    ];
    for (checks, said) in cases {
        let store = healthy();
        flaky(&store, "Change");
        let script = script_with_sign_in(checks);
        let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
        assert!(
            refused(&applied).contains(said),
            "{said}: {:?}",
            applied.applied
        );
        assert_eq!(
            carrying(&script, "write-value"),
            Vec::<String>::new(),
            "{said}"
        );
        assert_eq!(
            pollster::block_on(store.inner.action(&key())).unwrap(),
            None
        );
    }
}

#[test]
fn the_record_is_written_before_the_call_so_a_failed_or_raced_write_makes_no_call() {
    let config = config(TOKEN_WRITE);
    for (fault, said) in [
        (Fault::WritesActions, "could not be written"),
        (Fault::ActionsConflict, "another apply recorded"),
        (Fault::ReadsActions, "could not be read"),
    ] {
        let store = Failing { fault, ..healthy() };
        flaky(&store, "Change");
        let script = script_with_sign_in(checked("h1", 1));
        let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
        assert!(refused(&applied).contains(said), "{fault:?}");
        assert_eq!(
            carrying(&script, "write-value"),
            Vec::<String>::new(),
            "{fault:?}"
        );
    }
    let store = healthy();
    flaky(&store, "Change");
    let crashed = herald_core::RerunRecord {
        key: key(),
        operator: "octocat".to_owned(),
        credential: "HERALD_RERUN".to_owned(),
        at: NOON,
        reruns: 0,
        outcome: Reran::Calling,
    };
    pollster::block_on(store.inner.put_action(&crashed, None)).unwrap();
    let script = script_with_sign_in(checked("h1", 1));
    let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
    assert!(refused(&applied).contains("may or may not have reached"));
    assert_eq!(carrying(&script, "write-value"), Vec::<String>::new());
}

#[test]
fn a_lost_answer_is_read_back_never_asked_again_and_a_redirect_is_not_followed() {
    let config = config(TOKEN_WRITE);
    let cases = [
        ((502, ""), 2, Done::Rerun, Reran::Rerun),
        ((502, ""), 1, Done::Unsettled, Reran::Unsettled),
        ((302, ""), 1, Done::Unsettled, Reran::Unsettled),
        ((408, ""), 1, Done::Unsettled, Reran::Unsettled),
        ((429, ""), 1, Done::Unsettled, Reran::Unsettled),
        (
            (403, "{}"),
            1,
            Done::Declined(403),
            Reran::Refused { status: 403 },
        ),
    ];
    for ((status, body), back, expected, recorded) in cases {
        let store = healthy();
        flaky(&store, "Change");
        let mut call = vec![(CALL, status, body.to_owned())];
        if recorded != (Reran::Refused { status: 403 }) {
            call.extend(job_read(back, "failure"));
        }
        let script = scripted(&[planned(), signed_in(), checked("h1", 1), call])
            .headed(&[("Location", "https://elsewhere.example/taken")]);
        let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
        assert_eq!(done(&applied), std::slice::from_ref(&expected), "{status}");
        assert!(
            script
                .seen()
                .iter()
                .all(|line| !line.contains("elsewhere.example")),
            "{status}"
        );
        let record = pollster::block_on(store.inner.action(&key()))
            .unwrap()
            .unwrap();
        assert_eq!(record.value.outcome, recorded);
        let again = scripted(&[
            planned(),
            signed_in(),
            checked("h1", 1),
            vec![(CALL, 201, String::new())],
        ]);
        let applied = apply((&config, &store, &secrets()), &again, (CHOSEN, None));
        let asked = !carrying(&again, "write-value").is_empty();
        assert_eq!(
            asked,
            record.value.retried(),
            "{status}: {:?}",
            applied.applied
        );
    }
}

#[test]
fn a_job_the_plan_does_not_hold_signs_nobody_in_and_an_apply_that_cannot_record_asks_nothing() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let script = scripted(&[planned()]);
    let applied = apply(
        (&config, &store, &secrets()),
        &script,
        (&[("420", "h0"), ("9", "h1"), ("42", "h1")], None),
    );
    assert_eq!(
        done(&applied),
        [
            Done::Refused(UNPLANNED.to_owned()),
            Done::Refused(UNPLANNED.to_owned()),
            Done::Refused(UNPLANNED.to_owned())
        ]
    );
    assert_eq!(applied.told, Vec::<String>::new());
    assert_eq!(script.seen().len(), 2);
    assert_eq!(carrying(&script, "write-value"), Vec::<String>::new());
    let unkept = Failing {
        fault: Fault::KeepsNoActions,
        ..healthy()
    };
    let none = Script::answering(&[]);
    let applied = apply((&config, &unkept, &secrets()), &none, (CHOSEN, None));
    assert!(matches!(applied.applied, Err(Stop::Store(ref reason)) if reason == UNKEPT));
    let applied = apply((&config, &store, &secrets()), &none, (&[], None));
    assert!(matches!(applied.applied, Err(Stop::Source(ref reason)) if reason == UNCHOSEN));
    assert_eq!(none.seen(), Vec::<String>::new());
}

#[test]
fn a_sign_in_that_ends_or_a_write_credential_holding_the_read_secret_re_runs_nothing() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let declined = scripted(&[
        planned(),
        vec![
            signed_in()[0].clone(),
            (SIGNED, 200, json!({ "error": "access_denied" }).to_string()),
        ],
    ]);
    let applied = apply((&config, &store, &secrets()), &declined, (CHOSEN, None));
    assert!(refused(&applied).contains("declined"));
    assert!(declined.seen().iter().all(|line| !line.ends_with("/user")));
    let shared = secrets().with("HERALD_RERUN", "read-value");
    let script = scripted(&[planned()]);
    let applied = apply((&config, &store, &shared), &script, (CHOSEN, None));
    assert!(refused(&applied).contains("same secret"));
    assert_eq!(applied.told, Vec::<String>::new());
    let config = config_without_sign_in();
    let script = scripted(&[planned()]);
    let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
    assert_eq!(refused(&applied), herald_kinds::UNSIGNED);
}

fn config_without_sign_in() -> Config {
    config(r#"write = { token = { secret = "HERALD_RERUN" } }"#)
}

#[test]
fn a_call_the_budget_ends_before_is_recorded_unsent_and_a_settled_record_that_cannot_be_written_says_so()
 {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let script = script_with_sign_in(checked("h1", 1));
    let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, Some(12)));
    assert!(
        refused(&applied).contains("budget ended"),
        "{:?}",
        applied.applied
    );
    for most in [6, 7] {
        let store = healthy();
        flaky(&store, "Change");
        let script = script_with_sign_in(checked("h1", 1));
        let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, Some(most)));
        assert!(
            refused(&applied).contains(herald_sources_forge::UNINSTALLED),
            "{most}: {:?}",
            applied.applied
        );
        assert_eq!(carrying(&script, "write-value"), Vec::<String>::new());
    }
    let record = pollster::block_on(store.inner.action(&key()))
        .unwrap()
        .unwrap();
    assert_eq!(record.value.outcome, Reran::Unsent);
    let unsettled = Failing {
        fault: Fault::SettlesNoAction,
        ..healthy()
    };
    flaky(&unsettled, "Change");
    let script = scripted(&[
        planned(),
        signed_in(),
        checked("h1", 1),
        vec![(CALL, 201, String::new())],
    ]);
    let applied = apply((&config, &unsettled, &secrets()), &script, (CHOSEN, None));
    assert_eq!(done(&applied), [Done::Unrecorded(Reran::Rerun)]);
}

#[test]
fn a_source_that_cannot_be_made_ready_refuses_its_jobs_saying_why() {
    let sign_in = r#"sign_in = { client_id = "Iv23liClient" }"#;
    let app = r#"write = { app = "rerun" }
sign_in = { client_id = "Iv23liClient" }"#;
    let without = |name: &str| {
        let mut held = FakeSecrets::default();
        for (secret, value) in [
            ("HERALD_DISCORD_TOKEN", "discord-value"),
            ("HERALD_GITHUB_TOKEN", "read-value"),
            ("HERALD_RERUN", "write-value"),
            ("HERALD_RERUN_KEY", "the App's key"),
        ] {
            if secret != name {
                held = held.with(secret, value);
            }
        }
        held
    };
    let mut unappointed = config(app);
    unappointed.github_apps.remove("rerun");
    let cases = [
        (config(sign_in), secrets(), "write credential"),
        (
            config(TOKEN_WRITE),
            without("HERALD_GITHUB_TOKEN"),
            "read credential could not be read",
        ),
        (
            config(TOKEN_WRITE),
            without("HERALD_RERUN"),
            "write credential could not be read",
        ),
        (
            config(app),
            without("HERALD_RERUN_KEY"),
            "write App's key could not be read",
        ),
        (unappointed, secrets(), "write credential"),
    ];
    for (config, secrets, said) in cases {
        let store = healthy();
        flaky(&store, "Change");
        let script = scripted(&[planned()]);
        let applied = apply((&config, &store, &secrets), &script, (CHOSEN, None));
        assert!(
            refused(&applied).contains(said),
            "{said}: {:?}",
            applied.applied
        );
        assert_eq!(applied.told, Vec::<String>::new(), "{said}");
    }
    let store = healthy();
    flaky(&store, "Change");
    let script = scripted(&[planned()]);
    let config = config(TOKEN_WRITE);
    let applied = applied_by(
        (&config, &store, &secrets()),
        &script,
        (CHOSEN, None),
        Some("https://github.com"),
    );
    assert!(refused(&applied).contains("cannot be reached"));
}

#[test]
fn a_sign_in_that_cannot_be_asked_or_expires_signs_nobody_in() {
    let config = config(TOKEN_WRITE);
    let expiring = json!({ "device_code": "device-value", "user_code": "WDJB-MJHT",
        "verification_uri": "https://github.com/login/device", "interval": 5, "expires_in": 12 });
    let cases = [
        (vec![(DEVICE, 500, String::new())], "a device code"),
        (
            vec![signed_in()[0].clone(), (SIGNED, 500, "<html>".to_owned())],
            "sign-in's token",
        ),
        (
            vec![
                (DEVICE, 200, expiring.to_string()),
                (SIGNED, 200, json!({ "error": "slow_down" }).to_string()),
            ],
            herald_flows::UNFINISHED,
        ),
        (
            vec![
                signed_in()[0].clone(),
                signed_in()[2].clone(),
                (USER, 500, String::new()),
            ],
            "the signed-in operator",
        ),
    ];
    for (signing, said) in cases {
        let store = healthy();
        flaky(&store, "Change");
        let script = scripted(&[planned(), signing]);
        let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
        assert!(
            refused(&applied).contains(said),
            "{said}: {:?}",
            applied.applied
        );
        assert_eq!(carrying(&script, "write-value"), Vec::<String>::new());
    }
}

#[test]
fn a_source_reading_as_an_app_is_held_apart_from_its_write_credential_by_the_app_s_key() {
    let config = config_reading(r#"app = "reader""#, TOKEN_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let minted = json!({ "token": "reader-value", "expires_at": "2099-01-01T00:00:00Z" });
    let script = scripted(&[
        vec![
            (
                "/repos/o/open/installation",
                200,
                json!({ "id": 3 }).to_string(),
            ),
            (
                "/app/installations/3/access_tokens",
                201,
                minted.to_string(),
            ),
        ],
        planned(),
    ]);
    let shared = secrets().with("HERALD_RERUN", "the reading App's key");
    let applied = apply((&config, &store, &shared), &script, (CHOSEN, None));
    assert_eq!(refused(&applied), herald_flows::UNSEPARATED);
    let tokenless = config_reading("", TOKEN_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let script = scripted(&[
        planned(),
        signed_in(),
        checked("h1", 1),
        vec![(CALL, 201, String::new())],
    ]);
    let applied = apply((&tokenless, &store, &secrets()), &script, (CHOSEN, None));
    assert_eq!(done(&applied), [Done::Rerun]);
    assert_eq!(carrying(&script, "read-value"), Vec::<String>::new());
}

mod apps;
mod revoked;

#[test]
fn a_job_of_a_source_the_config_applied_under_does_not_name_is_refused_as_unwritten() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let mut unnamed = config.clone();
    unnamed.sources.clear();
    let script = scripted(&[planned()]);
    let applied = applied_from(
        &config,
        (&unnamed, &store, &secrets()),
        &script,
        (CHOSEN, None),
        None,
    );
    assert_eq!(refused(&applied), herald_kinds::UNWRITTEN);
    assert!(applied.told.is_empty() && applied.bases.is_empty());
    assert_eq!(script.seen().len(), 2, "{:?}", script.seen());
}

#[test]
fn a_rule_that_plans_no_re_run_applies_nothing_and_says_so() {
    let mut config = config(TOKEN_WRITE);
    config.rules.get_mut("flaky").unwrap().rerun = false;
    let store = healthy();
    let script = scripted(&[]);
    let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
    assert!(
        matches!(applied.applied, Err(Stop::Source(ref why)) if why.contains("plans no re-run")),
        "{:?}",
        applied.applied
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn a_sign_in_is_asked_after_until_the_moment_its_code_expires() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let mut signing = signed_in();
    signing[0].2 = json!({ "device_code": "device-value", "user_code": "WDJB-MJHT",
        "verification_uri": "https://github.com/login/device", "interval": 5, "expires_in": 10 })
    .to_string();
    let script = scripted(&[
        planned(),
        signing,
        checked("h1", 1),
        vec![(CALL, 201, String::new())],
    ]);
    let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
    assert_eq!(done(&applied), [Done::Rerun]);
    assert_eq!(applied.waited, 2);
}
