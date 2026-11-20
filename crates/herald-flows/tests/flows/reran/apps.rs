//! A write App's token, minted for one call alone, after every check and
//! herald's record, narrowed to re-running jobs of the one repository, and
//! never for an installation or an App a source reads through (ADR-0068).

use herald_core::Reran;
use herald_flows::{Done, SHARED_INSTALLATION};
use herald_ports::Actions as _;
use serde_json::{Value, json};

use super::{
    Answers, CALL, CHOSEN, REPOSITORY, SIGNED, apply, carrying, checked, config, config_reading,
    done, key, planned, refused, scripted, secrets, signed_in,
};
use crate::boards::flaky;
use crate::harness::{Failing, Fault, healthy};

const APP_WRITE: &str = r#"write = { app = "rerun" }
sign_in = { client_id = "Iv23liClient" }"#;

const INSTALLATION: &str = "/repos/o/open/installation";

/// A second source, reading `o/other` as the App `reader`.
const OTHER_READER: &str = r#"[sources.other]
kind = "github"
repository = "o/other"
app = "reader""#;

/// The App `reader`'s minting for the source `other`: installation 3 of
/// the App 30.
fn other_minted() -> Answers {
    let token = json!({ "token": "reader-value", "expires_at": "2099-01-01T00:00:00Z" });
    vec![
        (
            "/repos/o/other/installation",
            200,
            json!({ "id": 3, "app_id": 30 }).to_string(),
        ),
        ("/app/installations/3/access_tokens", 201, token.to_string()),
    ]
}

/// A minted token's answer, holding `permissions` on `open` alone.
fn token_holding(permissions: &Value) -> String {
    json!({ "token": "minted-value", "expires_at": "2099-01-01T00:00:00Z",
        "permissions": permissions, "repositories": [{ "name": "open" }] })
    .to_string()
}

/// The access tokens' path of the installation `id`.
fn tokens_of(id: u64) -> &'static str {
    match id {
        3 => "/app/installations/3/access_tokens",
        _ => "/app/installations/7/access_tokens",
    }
}

/// The write App's minting: its installation `id` of the App `app`, then
/// its token, narrowed to `actions: write` on `open`.
fn minted_of(id: u64, app: u64) -> Answers {
    vec![
        (
            INSTALLATION,
            200,
            json!({ "id": id, "app_id": app }).to_string(),
        ),
        (
            tokens_of(id),
            201,
            token_holding(&json!({ "actions": "write", "metadata": "read" })),
        ),
    ]
}

/// [`minted_of`] the write App 40.
fn minted(id: u64) -> Answers {
    minted_of(id, 40)
}

/// Whether any request of `script` was the write App's, carrying its JWT.
fn asserted(script: &crate::harness::Script) -> bool {
    script
        .seen()
        .iter()
        .any(|line| line.ends_with(INSTALLATION) || line.contains("/app/installations/"))
}

#[test]
fn a_write_app_mints_a_token_for_the_one_repository_after_every_check_and_calls_with_it() {
    let config = config(APP_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let script = scripted(&[
        planned(),
        signed_in(),
        checked("h1", 1),
        minted(7),
        vec![(CALL, 201, String::new())],
    ]);
    let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
    assert_eq!(done(&applied), [Done::Rerun]);
    assert_eq!(
        carrying(&script, "minted-value"),
        [format!("POST https://api.github.com{CALL}")]
    );
    let seen = script.seen();
    let mint = seen
        .iter()
        .position(|line| line.ends_with("/access_tokens"))
        .unwrap();
    assert!(
        seen[..mint]
            .iter()
            .any(|line| line.ends_with("/actions/runs/42")),
        "{seen:?}"
    );
    assert_eq!(
        script.bodies()[mint],
        herald_wire::github_app::rerunning("open").body().as_bytes()
    );
    let record = pollster::block_on(store.inner.action(&key()))
        .unwrap()
        .unwrap();
    assert_eq!(record.value.credential, "rerun");
}

#[test]
fn a_write_app_that_cannot_mint_records_the_call_unsent_so_a_later_apply_asks_again() {
    let config = config(APP_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let unminted = scripted(&[
        planned(),
        signed_in(),
        checked("h1", 1),
        vec![(INSTALLATION, 404, "{}".to_owned())],
    ]);
    let applied = apply((&config, &store, &secrets()), &unminted, (CHOSEN, None));
    assert!(refused(&applied).contains("could not be minted"));
    let record = pollster::block_on(store.inner.action(&key()))
        .unwrap()
        .unwrap();
    assert_eq!(record.value.outcome, Reran::Unsent);
    let again = scripted(&[
        planned(),
        signed_in(),
        checked("h1", 1),
        minted(7),
        vec![(CALL, 201, String::new())],
    ]);
    let applied = apply((&config, &store, &secrets()), &again, (CHOSEN, None));
    assert_eq!(done(&applied), [Done::Rerun]);
}

#[test]
fn an_operator_without_the_role_or_who_declines_causes_no_request_of_the_write_app() {
    let config = config(APP_WRITE);
    let unwritten = json!({ "permissions": { "pull": true } }).to_string();
    let declined = vec![
        signed_in()[0].clone(),
        (SIGNED, 200, json!({ "error": "access_denied" }).to_string()),
    ];
    for (signing, said) in [
        (
            [signed_in(), vec![(REPOSITORY, 200, unwritten)]].concat(),
            "holds read or none",
        ),
        (declined, "declined"),
    ] {
        let store = healthy();
        flaky(&store, "Change");
        let script = scripted(&[planned(), signing]);
        let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
        assert!(refused(&applied).contains(said), "{:?}", applied.applied);
        assert!(!asserted(&script), "{said}: {:?}", script.seen());
        assert_eq!(
            pollster::block_on(store.inner.action(&key())).unwrap(),
            None
        );
    }
}

#[test]
fn a_write_app_minted_for_the_installation_the_source_reads_through_makes_no_call() {
    let config = config_reading(r#"app = "reader""#, APP_WRITE);
    let read = json!({ "token": "reader-value", "expires_at": "2099-01-01T00:00:00Z" });
    let reading = vec![
        (
            INSTALLATION,
            200,
            json!({ "id": 3, "app_id": 30 }).to_string(),
        ),
        ("/app/installations/3/access_tokens", 201, read.to_string()),
    ];
    for (id, expected) in [
        (3, Done::Refused(SHARED_INSTALLATION.to_owned())),
        (7, Done::Rerun),
    ] {
        let store = healthy();
        flaky(&store, "Change");
        let mut call = minted(id);
        if id == 7 {
            call.push((CALL, 201, String::new()));
        }
        let script = scripted(&[
            reading.clone(),
            planned(),
            signed_in(),
            checked("h1", 1),
            call,
        ]);
        let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
        assert_eq!(done(&applied), std::slice::from_ref(&expected), "{id}");
        let called = !carrying(&script, "minted-value").is_empty();
        assert_eq!(called, id == 7, "{id}");
    }
}

#[test]
fn a_write_app_minted_for_another_source_s_installation_or_app_makes_no_call() {
    let config = config(&format!("{APP_WRITE}\n{OTHER_READER}"));
    let fixed = config_reading(
        super::TOKEN_READ,
        &format!(
            "write = {{ app = \"rerun\", installation = 7 }}\nsign_in = {{ client_id = \"Iv23liClient\" }}\n{OTHER_READER}"
        ),
    );
    let fixed_app = |app: u64| -> Answers {
        vec![
            (
                tokens_of(7),
                201,
                token_holding(&json!({ "actions": "write" })),
            ),
            (
                "/app/installations/7",
                200,
                json!({ "id": 7, "app_id": app }).to_string(),
            ),
        ]
    };
    let refused_call = Done::Refused(SHARED_INSTALLATION.to_owned());
    for (config, mint, expected) in [
        (&config, minted_of(3, 40), refused_call.clone()),
        (&config, minted_of(7, 30), refused_call.clone()),
        (&fixed, fixed_app(30), refused_call),
        (&config, minted_of(7, 40), Done::Rerun),
        (&fixed, fixed_app(40), Done::Rerun),
    ] {
        let store = healthy();
        flaky(&store, "Change");
        let mut call = mint;
        if expected == Done::Rerun {
            call.push((CALL, 201, String::new()));
        }
        let script = scripted(&[
            other_minted(),
            planned(),
            signed_in(),
            checked("h1", 1),
            call,
        ]);
        let applied = apply((config, &store, &secrets()), &script, (CHOSEN, None));
        assert_eq!(
            done(&applied),
            std::slice::from_ref(&expected),
            "{:?}",
            script.seen()
        );
        let called = !carrying(&script, "minted-value").is_empty();
        assert_eq!(called, expected == Done::Rerun, "{:?}", script.seen());
    }
}

#[test]
fn a_write_app_s_token_holding_more_than_re_running_is_never_used() {
    let config = config(APP_WRITE);
    let wider = json!({ "actions": "write", "contents": "write" });
    for (permissions, most, revoking, said) in [
        (
            wider.clone(),
            None,
            204,
            "so herald never uses it; it was revoked",
        ),
        (json!({ "actions": "read" }), None, 204, "; it was revoked"),
        (wider.clone(), Some(14), 204, "; it was revoked"),
        (wider, None, 500, ", and its revocation failed: "),
    ] {
        let store = healthy();
        flaky(&store, "Change");
        let wide = vec![
            (
                INSTALLATION,
                200,
                json!({ "id": 7, "app_id": 40 }).to_string(),
            ),
            (tokens_of(7), 201, token_holding(&permissions)),
            ("/installation/token", revoking, String::new()),
        ];
        let script = scripted(&[planned(), signed_in(), checked("h1", 1), wide]);
        let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, most));
        assert!(
            refused(&applied).contains(said),
            "{permissions} {most:?}: {:?}",
            applied.applied
        );
        assert_eq!(
            carrying(&script, "minted-value"),
            ["DELETE https://api.github.com/installation/token"]
        );
        let record = pollster::block_on(store.inner.action(&key()))
            .unwrap()
            .unwrap();
        assert_eq!(record.value.outcome, Reran::Unsent);
    }
    let store = healthy();
    flaky(&store, "Change");
    let lookup = json!({ "id": 7, "app_id": 40 }).to_string();
    let script = scripted(&[
        planned(),
        signed_in(),
        checked("h1", 1),
        vec![(INSTALLATION, 200, lookup)],
    ]);
    let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, Some(13)));
    assert!(
        refused(&applied).contains("13 of the 13 outbound requests"),
        "the mint is the fourteenth request: {:?}",
        applied.applied
    );
}

#[test]
fn a_record_that_cannot_be_rewritten_as_unsent_says_it_still_reads_as_calling() {
    let config = config(APP_WRITE);
    let store = Failing {
        fault: Fault::SettlesNoAction,
        ..healthy()
    };
    flaky(&store, "Change");
    let unminted = scripted(&[
        planned(),
        signed_in(),
        checked("h1", 1),
        vec![(INSTALLATION, 404, "{}".to_owned())],
    ]);
    let applied = apply((&config, &store, &secrets()), &unminted, (CHOSEN, None));
    assert_eq!(done(&applied), [Done::Unrecorded(Reran::Unsent)]);
    let record = pollster::block_on(store.inner.action(&key()))
        .unwrap()
        .unwrap();
    assert_eq!(record.value.outcome, Reran::Calling);
}

#[test]
fn each_job_of_one_write_app_source_is_called_with_a_token_minted_for_it_alone() {
    let config = config(APP_WRITE);
    let store = healthy();
    flaky(&store, "Change");
    let runs = json!({ "workflow_runs": [
        { "id": 42, "html_url": "https://github.com/o/open/actions/runs/42" },
        { "id": 43, "html_url": "https://github.com/o/open/actions/runs/43" }
    ] });
    let jobs = |id: u64| json!({ "jobs": [{ "id": id, "name": "test", "conclusion": "failure" }] });
    let second = vec![
        (
            REPOSITORY,
            200,
            json!({ "permissions": { "push": true } }).to_string(),
        ),
        (
            "/repos/o/open/pulls/1",
            200,
            json!({ "state": "open", "head": { "sha": "h1" } }).to_string(),
        ),
        (
            "/repos/o/open/actions/jobs/430",
            200,
            json!({ "run_id": 43, "head_sha": "h1", "status": "completed", "conclusion": "failure" })
                .to_string(),
        ),
        (
            "/repos/o/open/actions/runs/43",
            200,
            json!({ "run_attempt": 1 }).to_string(),
        ),
    ];
    let script = scripted(&[
        vec![
            (super::RUNS, 200, runs.to_string()),
            (super::JOBS, 200, jobs(420).to_string()),
            (
                "/repos/o/open/actions/runs/43/jobs?filter=latest&per_page=100",
                200,
                jobs(430).to_string(),
            ),
        ],
        signed_in(),
        checked("h1", 1),
        minted(7),
        vec![(CALL, 201, String::new())],
        second,
        minted(7),
        vec![("/repos/o/open/actions/jobs/430/rerun", 201, String::new())],
    ]);
    let applied = apply(
        (&config, &store, &secrets()),
        &script,
        (&[("420", "h1"), ("430", "h1")], None),
    );
    assert_eq!(done(&applied), [Done::Rerun, Done::Rerun]);
    assert_eq!(
        carrying(&script, "minted-value"),
        [
            format!("POST https://api.github.com{CALL}"),
            "POST https://api.github.com/repos/o/open/actions/jobs/430/rerun".to_owned(),
        ]
    );
    let seen = script.seen();
    let bodies = script.bodies();
    let mints: Vec<&[u8]> = seen
        .iter()
        .zip(&bodies)
        .filter(|(line, _)| line.ends_with("/access_tokens"))
        .map(|(_, body)| body.as_slice())
        .collect();
    let narrowed = herald_wire::github_app::rerunning("open");
    assert_eq!(mints, [narrowed.body().as_bytes(); 2], "{seen:?}");
}
