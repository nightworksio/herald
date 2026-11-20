//! The operator's token revoked when the apply ends, wherever it arrived:
//! refused at sign-in, refused by a check, or used for a call (ADR-0068).

use herald_config::{Config, load};
use herald_flows::Done;
use serde_json::json;

use super::{Answers, CHOSEN, apply, done, refused, scripted, secrets};
use crate::boards::flaky_on;
use crate::harness::healthy;

const DEVICE: &str = "https://gitlab.com/oauth/authorize_device";
const SIGNED: &str = "https://gitlab.com/oauth/token";
const USER: &str = "https://gitlab.com/api/v4/user";
const INFO: &str = "https://gitlab.com/oauth/token/info";
const REVOKE: &str = "https://gitlab.com/oauth/revoke";
const PROJECT: &str = "https://gitlab.com/api/v4/projects/g%2Fopen";

fn gitlab() -> Config {
    let text = r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.lab]
kind = "gitlab"
repository = "g/open"
token = { secret = "HERALD_GITHUB_TOKEN" }
write = { token = { secret = "HERALD_RERUN" } }
sign_in = { client_id = "Iv23liClient" }
[views.kept]
sources = ["lab"]
mode = "kept"
reconcile = "1d"
[rules.flaky]
view = "kept"
flaky = { rerun = true }
every = "1h"
at = "00:00"
"#;
    load(
        "herald.toml",
        text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

/// The plan's answers: the failed pipeline 42 on `h1`, whose job `test`,
/// 420, failed.
fn planned() -> Answers {
    let pipelines = json!([{ "id": 42, "web_url": "https://gitlab.com/g/open/-/pipelines/42" }]);
    let jobs = json!([{ "id": 420, "name": "test" }]);
    vec![
        (
            "/pipelines?sha=h1&status=failed&per_page=5",
            200,
            pipelines.to_string(),
        ),
        (
            "/pipelines/42/jobs?scope=failed&per_page=100",
            200,
            jobs.to_string(),
        ),
    ]
}

/// The sign-in's answers up to the operator's token, then the operator it
/// names and the scopes GitLab says it holds.
fn signed_in(scopes: &[&str]) -> Answers {
    vec![
        (
            DEVICE,
            200,
            json!({ "device_code": "device-value", "user_code": "WDJB-MJHT",
                "verification_uri": "https://gitlab.com/oauth/device" })
            .to_string(),
        ),
        (
            SIGNED,
            200,
            json!({ "access_token": "operator-value" }).to_string(),
        ),
        (USER, 200, json!({ "username": "maintainer" }).to_string()),
        (INFO, 200, json!({ "scope": scopes }).to_string()),
    ]
}

fn revoked() -> Answers {
    vec![(REVOKE, 200, "{}".to_owned())]
}

#[test]
fn an_operator_s_token_is_revoked_whether_it_was_refused_at_sign_in_or_by_a_check() {
    let config = gitlab();
    let unwritten = json!({ "permissions": { "project_access": { "access_level": 20 } } });
    let cases = [
        (
            [planned(), signed_in(&["api"]), revoked()].concat(),
            "can do more than read",
        ),
        (
            [
                planned(),
                signed_in(&["read_api"]),
                vec![(PROJECT, 200, unwritten.to_string())],
                revoked(),
            ]
            .concat(),
            "holds access level 20",
        ),
    ];
    for (answers, said) in cases {
        let store = healthy();
        flaky_on(&store, ("lab", "gitlab.com", "g/open"), "Change");
        let script = scripted(&[answers]);
        let applied = apply((&config, &store, &secrets()), &script, (CHOSEN, None));
        assert!(
            refused(&applied).contains(said),
            "{said}: {:?}",
            applied.applied
        );
        let seen = script.seen();
        assert_eq!(
            seen.last().map(String::as_str),
            Some("POST https://gitlab.com/oauth/revoke"),
            "{said}"
        );
        assert_eq!(
            script.bodies().last().cloned().map(String::from_utf8),
            Some(Ok("client_id=Iv23liClient&token=operator-value".to_owned())),
            "{said}"
        );
    }
    let store = healthy();
    flaky_on(&store, ("lab", "gitlab.com", "g/open"), "Change");
    let declined = scripted(&[
        planned(),
        vec![
            signed_in(&[])[0].clone(),
            (SIGNED, 200, json!({ "error": "access_denied" }).to_string()),
        ],
    ]);
    let applied = apply((&config, &store, &secrets()), &declined, (CHOSEN, None));
    assert!(matches!(done(&applied).as_slice(), [Done::Refused(_)]));
    assert!(
        declined
            .seen()
            .iter()
            .all(|line| !line.ends_with("/revoke"))
    );
}
