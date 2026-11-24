use herald_core::Permission;
use herald_kinds::Gesture;
use herald_ports::{Method, Secret};
use herald_testing::ScriptedHttp;
use jiff::Timestamp;

use super::*;
use crate::SourceOptions;

const WRITE_VALUE: &str = "glpat-write-value";

fn writer(forge: Forge, api: Option<&str>) -> ForgeMergeWriter {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        api: api.map(ToOwned::to_owned),
        ..SourceOptions::default()
    };
    ForgeMergeWriter {
        origin: Origin::new(forge, &options, "o/r"),
        graphql: crate::graphql::graphql_of(forge, &options),
        token: Token::Named(Secret::new("HERALD_MERGE", WRITE_VALUE)),
    }
}

fn entered(id: Option<&str>) -> Entered {
    Entered {
        number: 7,
        url: "https://forge.test/o/r/pull/7".to_owned(),
        id: id.map(ToOwned::to_owned),
        draft: false,
        head: "h7".to_owned(),
        gesture: Gesture {
            id: Some(70),
            by: "w".to_owned(),
            at: Timestamp::UNIX_EPOCH,
            head: Some("h7".to_owned()),
            named: None,
            role: Some(Permission::Write),
        },
        arrived: None,
        approved: false,
        whole: true,
    }
}

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn handed(
    writer: &ForgeMergeWriter,
    answers: Vec<Response>,
) -> (ScriptedHttp, Result<Handed, Unpolled>) {
    let http = ScriptedHttp::new(answers);
    let handed = pollster::block_on(writer.hand_off(&http, &entered(Some("PR_7"))));
    (http, handed)
}

fn bearer(request: &herald_ports::Request) -> Vec<String> {
    request
        .headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.expose())
        .collect()
}

#[test]
fn github_is_asked_to_enqueue_the_pull_request_expecting_its_head_with_the_write_credential() {
    let queued =
        json!({ "data": { "enqueuePullRequest": { "mergeQueueEntry": { "id": "MQE_1" } } } });
    for (api, endpoint) in [
        (None, "https://api.github.com/graphql"),
        (
            Some("https://ghe.example/api/v3"),
            "https://ghe.example/api/graphql",
        ),
        (
            Some("https://api.acme.ghe.com"),
            "https://api.acme.ghe.com/graphql",
        ),
    ] {
        let writer = writer(Forge::GitHub, api);
        let (http, handed) = handed(&writer, vec![answer(200, &queued)]);
        assert_eq!(handed.unwrap(), Handed::Queued);
        let request = &http.requests()[0];
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.url.expose(), endpoint);
        assert_eq!(bearer(request), [format!("Bearer {WRITE_VALUE}")]);
        assert_eq!(
            http.bodies()[0],
            json!({ "query": ENQUEUE, "variables": { "id": "PR_7", "head": "h7" } })
        );
    }
    let api = writer(Forge::GitHub, Some("https://ghe.example/api/v3"));
    assert_eq!(api.api(), "https://ghe.example/api/v3");
}

#[test]
fn github_s_answer_queues_refuses_or_is_lost_and_a_refusal_never_shows_the_credential() {
    let writer = writer(Forge::GitHub, None);
    let refused = json!({ "errors": [
        { "type": "UNPROCESSABLE", "message": format!("head moved for {WRITE_VALUE}") },
        { "message": "second\u{7}" }
    ] });
    let (_, said) = handed(&writer, vec![answer(200, &refused)]);
    assert_eq!(
        said.unwrap(),
        Handed::Refused(format!(
            "GitHub refused it: head moved for {HIDDEN}; second"
        ))
    );
    let (_, denied) = handed(
        &writer,
        vec![answer(
            403,
            &json!({ "message": "Resource not accessible by integration" }),
        )],
    );
    assert_eq!(
        denied.unwrap(),
        Handed::Refused(
            "the forge answered 403: Resource not accessible by integration".to_owned()
        )
    );
    let (_, bare) = handed(&writer, vec![answer(422, &json!({}))]);
    assert_eq!(
        bare.unwrap(),
        Handed::Refused("the forge answered 422".to_owned())
    );
    for lost in [
        answer(500, &json!({})),
        answer(502, &json!({})),
        answer(408, &json!({})),
        answer(429, &json!({})),
        answer(
            200,
            &json!({ "data": { "enqueuePullRequest": { "mergeQueueEntry": null } } }),
        ),
        answer(200, &json!({ "data": { "enqueuePullRequest": null } })),
        answer(200, &json!({ "data": null })),
        Response {
            status: 200,
            headers: Vec::new(),
            body: b"not json".to_vec(),
        },
    ] {
        let status = lost.status;
        let (_, handed) = handed(&writer, vec![lost]);
        assert_eq!(handed.unwrap(), Handed::Lost, "{status}");
    }
    let (_, unanswered) = handed(&writer, Vec::new());
    assert_eq!(unanswered.unwrap(), Handed::Lost);
}

#[test]
fn a_pull_request_github_named_no_node_id_for_is_never_sent() {
    let writer = writer(Forge::GitHub, None);
    let http = ScriptedHttp::new(Vec::new());
    let unsent = pollster::block_on(writer.hand_off(&http, &entered(None)));
    assert!(
        matches!(unsent, Err(Unpolled::Unreadable(ref why)) if why.contains("no node id")),
        "{unsent:?}"
    );
    assert_eq!(http.requests().len(), 0);
}

#[test]
fn gitlab_adds_the_merge_request_to_its_train_with_its_head_s_sha_once_its_checks_pass() {
    let writer = writer(Forge::GitLab, Some("https://git.example/api/v4"));
    for status in [201, 202] {
        let (http, handed) = handed(&writer, vec![answer(status, &json!([{ "id": 1 }]))]);
        assert_eq!(handed.unwrap(), Handed::Queued);
        let request = &http.requests()[0];
        assert_eq!(request.method, Method::Post);
        assert_eq!(
            request.url.expose(),
            "https://git.example/api/v4/projects/o%2Fr/merge_trains/merge_requests/7"
        );
        assert_eq!(bearer(request), [format!("Bearer {WRITE_VALUE}")]);
        assert_eq!(http.bodies()[0], json!({ "sha": "h7", "auto_merge": true }));
    }
    let (_, refused) = handed(
        &writer,
        vec![answer(
            400,
            &json!({ "message": { "sha": ["does not match"] } }),
        )],
    );
    assert_eq!(
        refused.unwrap(),
        Handed::Refused(r#"the forge answered 400: {"sha":["does not match"]}"#.to_owned())
    );
    let (_, conflict) = handed(
        &writer,
        vec![answer(
            409,
            &json!({ "error": format!("token {WRITE_VALUE} rejected") }),
        )],
    );
    assert_eq!(
        conflict.unwrap(),
        Handed::Refused(format!("the forge answered 409: token {HIDDEN} rejected"))
    );
    for lost in [500, 429] {
        let (_, handed) = handed(&writer, vec![answer(lost, &json!({}))]);
        assert_eq!(handed.unwrap(), Handed::Lost, "{lost}");
    }
    let (_, unanswered) = handed(&writer, Vec::new());
    assert_eq!(unanswered.unwrap(), Handed::Lost);
}

#[test]
fn forgejo_has_no_queue_to_hand_a_pull_request_to() {
    let writer = writer(Forge::Forgejo, Some("https://code.example/api/v1"));
    let http = ScriptedHttp::new(Vec::new());
    let unsent = pollster::block_on(writer.hand_off(&http, &entered(Some("PR_7"))));
    assert!(
        matches!(unsent, Err(Unpolled::Unreadable(ref why)) if why == crate::options::FORGEJO_QUEUELESS),
        "{unsent:?}"
    );
    assert_eq!(http.requests().len(), 0);
    let answer = Ok(answer(500, &json!({})));
    assert_eq!(writer.handed(&answer), Handed::Lost);
}

#[test]
fn a_refusal_is_shown_short_without_control_characters_or_the_credential() {
    let long = "x".repeat(MOST_SAID + 50);
    assert_eq!(shown(&long, None).chars().count(), MOST_SAID);
    assert_eq!(shown("a\nb\tc", None), "abc");
    assert_eq!(
        shown("use abc here", Some("abc")),
        format!("use {HIDDEN} here")
    );
    assert_eq!(shown("nothing", Some("")), "nothing");
    assert!(refusing(400) && refusing(499) && !refusing(408) && !refusing(429));
    assert!(!refusing(399) && !refusing(500));
}

#[test]
fn a_minted_write_credential_is_hidden_from_a_refusal_too() {
    let mut writer = writer(Forge::GitLab, None);
    let minted = herald_kinds::Minted::default();
    minted.refresh(Some(Secret::new("github_apps.merger", "ghs_minted")));
    writer.token = Token::Minted(std::sync::Arc::new(minted));
    let (_, refused) = handed(
        &writer,
        vec![answer(
            403,
            &json!({ "message": "ghs_minted is read-only" }),
        )],
    );
    assert_eq!(
        refused.unwrap(),
        Handed::Refused(format!("the forge answered 403: {HIDDEN} is read-only"))
    );
    writer.token = Token::Minted(std::sync::Arc::new(herald_kinds::Minted::default()));
    assert_eq!(writer.exposed(), None);
}
