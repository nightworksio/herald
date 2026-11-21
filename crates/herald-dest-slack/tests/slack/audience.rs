//! Who may read a Slack route's channel, read right before a run's first
//! advisories section is sent there (ADR-0067).

use herald_dest_slack::{OrgShared, RouteOptions, SCOPELESS, Slack, UNCHECKED};
use herald_kinds::{Audience, Reach};
use herald_ports::{Credential, Response, Secret};
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

fn bot(channel: Option<&str>) -> Slack {
    Slack::new(
        Credential::Bot(Secret::new("HERALD_SLACK_TOKEN", "test-token")),
        RouteOptions {
            channel: channel.map(ToOwned::to_owned),
            ..RouteOptions::default()
        },
        None,
    )
}

fn answered(body: &Value) -> Vec<Response> {
    vec![Response {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }]
}

fn info(org: bool) -> Value {
    json!({ "ok": true, "channel": {
        "id": "G01", "is_private": true, "is_ext_shared": false, "is_shared": org,
        "is_org_shared": org, "is_pending_ext_shared": false
    } })
}

fn audience(slack: &Slack, answers: Vec<Response>) -> (Audience, Vec<String>) {
    let http = ScriptedHttp::new(answers);
    let audience = pollster::block_on(slack.audience(&http));
    (
        audience,
        http.asked().into_iter().map(|(url, _)| url).collect(),
    )
}

#[test]
fn an_organization_shared_channel_is_public_unless_org_shared_says_private() {
    let (private, asked) = audience(&bot(Some("G01")), answered(&info(false)));
    assert_eq!(private, Audience::Private);
    assert_eq!(
        asked,
        ["https://slack.com/api/conversations.info?channel=G01"]
    );
    let (org, _) = audience(&bot(Some("G01")), answered(&info(true)));
    assert!(matches!(org, Audience::Public(_)), "{org:?}");
    let accepted = bot(Some("G01")).org_shared(OrgShared::Private);
    let (private, _) = audience(&accepted, answered(&info(true)));
    assert_eq!(private, Audience::Private);
    assert!(!accepted.keeps_edits());
}

#[test]
fn a_channel_slack_will_not_describe_is_never_taken_as_private() {
    let (scopeless, _) = audience(
        &bot(Some("G01")),
        answered(&json!({ "ok": false, "error": "missing_scope" })),
    );
    assert_eq!(scopeless, Audience::Unread(SCOPELESS.to_owned()));
    for (slack, answers) in [
        (
            bot(Some("G01")),
            answered(&json!({ "ok": false, "error": "channel_not_found" })),
        ),
        (bot(Some("G01")), answered(&json!({}))),
        (
            bot(Some("G01")),
            answered(&json!({ "ok": true, "channel": {} })),
        ),
        (bot(Some("G01")), Vec::new()),
        (bot(None), Vec::new()),
    ] {
        let (unread, _) = audience(&slack, answers);
        assert!(matches!(unread, Audience::Unread(_)), "{unread:?}");
    }
    let webhook = Slack::new(
        Credential::Webhook(Secret::new(
            "HERALD_SLACK_WEBHOOK",
            "https://hooks.slack.com/services/T/B/x",
        )),
        RouteOptions::default(),
        None,
    );
    let (refused, asked) = audience(&webhook, Vec::new());
    assert_eq!(
        (refused, asked),
        (Audience::Unread(UNCHECKED.to_owned()), Vec::<String>::new())
    );
}
