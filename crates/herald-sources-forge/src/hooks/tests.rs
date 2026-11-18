use herald_ports::{HttpError, Response};
use herald_wire::SourceError;
use serde_json::{Value, json};

use super::*;
use herald_testing::ScriptedHttp;

const API: &str = "https://gitlab.com/api/v4";
const URL: &str = "https://herald.example/hooks/group";

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn register(scripted: &ScriptedHttp) -> Result<Registered, Unpolled> {
    registering(scripted, (false, false))
}

fn registering(scripted: &ScriptedHttp, signalled: (bool, bool)) -> Result<Registered, Unpolled> {
    let token = Token::Named(Secret::new("GITLAB_TOKEN", "glpat"));
    pollster::block_on(registered(
        scripted,
        (API, Some(&token)),
        ("7", signalled),
        (URL, &Secret::new("HERALD_WEBHOOK", "hook-secret")),
    ))
}

#[test]
fn a_member_with_a_hook_to_herald_keeps_it_and_one_without_is_given_one() {
    let found = ScriptedHttp::new(vec![answer(
        200,
        &json!([{ "url": "https://elsewhere.example" }, { "url": URL }]),
    )]);
    assert_eq!(register(&found), Ok(Registered::Found));
    assert_eq!(found.requests().len(), 1);
    let made = ScriptedHttp::new(vec![
        answer(200, &json!([{ "url": "https://elsewhere.example" }])),
        answer(201, &json!({ "id": 1 })),
    ]);
    assert_eq!(register(&made), Ok(Registered::Made));
    let asked: Vec<String> = made.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        asked,
        [
            format!("{API}/projects/7/hooks?per_page=100"),
            format!("{API}/projects/7/hooks"),
        ]
    );
    let posted = &made.requests()[1];
    assert_eq!(
        serde_json::from_slice::<Value>(&posted.body).unwrap(),
        json!({
            "url": URL,
            "token": "hook-secret",
            "releases_events": true,
            "tag_push_events": true,
            "push_events": false,
            "pipeline_events": false,
            "vulnerability_events": false,
        })
    );
    assert_eq!(posted.method, Method::Post);
    assert!(posted.sealed);
}

#[test]
fn a_member_announcing_runs_is_given_a_hook_sending_its_pipelines() {
    let made = ScriptedHttp::new(vec![
        answer(200, &json!([])),
        answer(201, &json!({ "id": 1 })),
    ]);
    assert_eq!(registering(&made, (true, false)), Ok(Registered::Made));
    let posted: Value = serde_json::from_slice(&made.requests()[1].body).unwrap();
    assert_eq!(posted["pipeline_events"], json!(true));
    assert_eq!(posted["vulnerability_events"], json!(false));
    assert_eq!(posted["releases_events"], json!(true));
}

#[test]
fn a_member_announcing_alerts_is_given_a_hook_sending_its_vulnerabilities() {
    let made = ScriptedHttp::new(vec![
        answer(200, &json!([])),
        answer(201, &json!({ "id": 1 })),
    ]);
    assert_eq!(registering(&made, (false, true)), Ok(Registered::Made));
    let posted: Value = serde_json::from_slice(&made.requests()[1].body).unwrap();
    assert_eq!(posted["pipeline_events"], json!(false));
    assert_eq!(posted["vulnerability_events"], json!(true));
}

#[test]
fn a_hook_that_cannot_be_listed_or_registered_says_why() {
    assert_eq!(
        register(&ScriptedHttp::new(vec![answer(403, &json!({}))])),
        Err(Unpolled::unreadable(SourceError::Answered {
            what: HOOKS,
            status: 403
        }))
    );
    assert_eq!(
        register(&ScriptedHttp::new(vec![
            answer(200, &json!([])),
            answer(403, &json!({}))
        ])),
        Err(Unpolled::unreadable(SourceError::Answered {
            what: REGISTERED,
            status: 403
        }))
    );
    assert!(register(&ScriptedHttp::new(vec![answer(200, &json!({}))])).is_err());
    assert_eq!(
        register(&ScriptedHttp::new(Vec::new())),
        Err(Unpolled::unreachable(HttpError::TimedOut))
    );
    assert_eq!(
        register(&ScriptedHttp::new(vec![answer(200, &json!([]))])),
        Err(Unpolled::unreachable(HttpError::TimedOut))
    );
}

fn scoped(scripted: &ScriptedHttp) -> Result<(), String> {
    let token = Token::Named(Secret::new("GITLAB_TOKEN", "glpat"));
    pollster::block_on(may_register(scripted, (API, Some(&token))))
}

#[test]
fn a_token_registers_webhooks_only_with_the_api_scope() {
    let api = ScriptedHttp::new(vec![answer(200, &json!({ "scopes": ["read_api", "api"] }))]);
    assert_eq!(scoped(&api), Ok(()));
    assert_eq!(
        api.asked()[0].0,
        format!("{API}/personal_access_tokens/self")
    );
    assert_eq!(
        scoped(&ScriptedHttp::new(vec![answer(
            200,
            &json!({ "scopes": ["read_api", "read_repository"] })
        )])),
        Err("its scopes are read_api, read_repository, and registering a project webhook needs the api scope".to_owned())
    );
    assert!(
        scoped(&ScriptedHttp::new(vec![answer(401, &json!({}))]))
            .unwrap_err()
            .contains("401")
    );
    assert!(scoped(&ScriptedHttp::new(vec![answer(200, &json!({}))])).is_err());
    assert!(scoped(&ScriptedHttp::new(Vec::new())).is_err());
}

#[test]
fn a_member_s_webhook_to_herald_is_found_without_registering_one() {
    let token = Token::Named(Secret::new("GITLAB_TOKEN", "glpat"));
    let present = |hooks: Value| {
        pollster::block_on(found(
            &ScriptedHttp::new(vec![answer(200, &hooks)]),
            (API, Some(&token)),
            "7",
            URL,
        ))
    };
    assert_eq!(present(json!([{ "url": URL }])), Ok(true));
    assert_eq!(present(json!([])), Ok(false));
}
