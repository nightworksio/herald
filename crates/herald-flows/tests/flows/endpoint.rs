use herald_core::{Command, DestinationName, Place};
use herald_flows::{Answered, Inbound, Interacted, Job, Worked, unworked};
use serde_json::{Value, json};

use crate::harness::{Fault, Script};
use crate::inbound::at;
use crate::interact::{
    APPLICATION, CHANNEL, GUILD, HOOK, KEY, ROOM, SLACK_CHANNEL, TOKEN, USER, World, discord,
    discord_replying, world,
};

fn signed_discord(body: &[u8]) -> Vec<(String, String)> {
    let stamp = at().as_second().to_string();
    let key = herald_core::key(herald_dest_discord::SCHEME, KEY).unwrap();
    let message = [stamp.as_bytes(), body].concat();
    let signature = [b"Ed25519".as_slice(), &key, &message].join(&b'|');
    vec![
        (
            "X-Signature-Ed25519".to_owned(),
            herald_core::hex(&signature),
        ),
        ("X-Signature-Timestamp".to_owned(), stamp),
    ]
}

fn signed_slack(body: &[u8]) -> Vec<(String, String)> {
    let stamp = at().as_second().to_string();
    let message = [format!("v0:{stamp}:").as_bytes(), body].concat();
    let signature = [b"HmacSha256".as_slice(), b"signing-secret", &message].join(&b'|');
    vec![
        (
            "X-Slack-Signature".to_owned(),
            format!("v0={}", herald_core::hex(&signature)),
        ),
        ("X-Slack-Request-Timestamp".to_owned(), stamp),
    ]
}

fn posted<'a>(path: &'a str, headers: &'a [(&'a str, &'a str)], body: &'a [u8]) -> Inbound<'a> {
    Inbound {
        method: "POST",
        path,
        query: None,
        headers,
        body,
    }
}

fn post(world: &World, path: &str, headers: &[(String, String)], body: &[u8]) -> Answered {
    let borrowed: Vec<(&str, &str)> = headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    world.answer(&posted(path, &borrowed, body))
}

fn interaction(kind: u64, data: &Value) -> Vec<u8> {
    json!({
        "id": "900",
        "token": "tok",
        "type": kind,
        "application_id": APPLICATION,
        "channel_id": CHANNEL,
        "guild_id": GUILD,
        "member": { "user": { "id": USER }, "roles": [] },
        "data": data,
    })
    .to_string()
    .into_bytes()
}

#[test]
fn a_discord_interaction_is_verified_then_pinged_completed_or_deferred_with_its_work() {
    let world = world();
    let path = "/interactions/discord";
    let ping = br#"{"type":1}"#;
    let ponged = post(&world, path, &signed_discord(ping), ping);
    assert_eq!(
        (ponged.reply.status, ponged.reply.body.as_slice()),
        (200, br#"{"type":1}"#.as_slice())
    );
    let unsigned = post(&world, path, &[], ping);
    assert_eq!(unsigned.reply.status, 401);
    let replayed = post(&world, path, &signed_discord(ping), ping);
    assert_eq!(replayed.reply.status, 409);
    let typing = interaction(
        4,
        &json!({ "name": "latest", "options": [{ "name": "project", "type": 3, "value": "ga", "focused": true }] }),
    );
    let completing = post(&world, path, &signed_discord(&typing), &typing);
    assert_eq!(
        String::from_utf8(completing.reply.body).unwrap(),
        r#"{"data":{"choices":[{"name":"gate","value":"gate"}]},"type":8}"#
    );
    let command = interaction(2, &json!({ "name": "latest" }));
    let deferred = post(&world, path, &signed_discord(&command), &command);
    assert_eq!(deferred.reply.status, 200);
    assert_eq!(deferred.reply.body, br#"{"data":{"flags":64},"type":5}"#);
    let Some(Job::Interact { destination, asked }) = deferred.job else {
        panic!("a command leaves its work");
    };
    assert_eq!(destination, DestinationName::new("discord"));
    assert_eq!(asked[0].0.command, Command::Latest);
    let shared = interaction(
        2,
        &json!({ "name": "changelog", "options": [{ "name": "share", "type": 5, "value": true }] }),
    );
    let public = post(&world, path, &signed_discord(&shared), &shared);
    assert_eq!(public.reply.body, br#"{"type":5}"#);
    let button = interaction(3, &json!({ "custom_id": "someone:else" }));
    assert_eq!(
        post(&world, path, &signed_discord(&button), &button)
            .reply
            .status,
        400
    );
    let broken = br#"{"type":2}"#;
    assert_eq!(
        post(&world, path, &signed_discord(broken), broken)
            .reply
            .status,
        400
    );
    let garbled = b"not json";
    assert_eq!(
        post(&world, path, &signed_discord(garbled), garbled)
            .reply
            .status,
        400
    );
}

fn form(text: &str) -> Vec<u8> {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("command", "/herald")
        .append_pair("text", text)
        .append_pair("channel_id", SLACK_CHANNEL)
        .append_pair("user_id", "U0123456789")
        .append_pair("response_url", HOOK)
        .finish()
        .into_bytes()
}

#[test]
fn a_slack_command_is_verified_then_answered_at_once_with_its_work_or_its_usage() {
    let world = world();
    let path = "/interactions/slack";
    let latest = form("latest gate");
    let answered = post(&world, path, &signed_slack(&latest), &latest);
    assert_eq!((answered.reply.status, answered.reply.body.len()), (200, 0));
    let Some(job) = answered.job else {
        panic!("a command leaves its work");
    };
    world.work(job);
    assert_eq!(
        world.script.seen(),
        ["POST «secret:the command's response URL»"]
    );
    let usage = form("deploy");
    let explained = post(&world, path, &signed_slack(&usage), &usage);
    assert!(
        String::from_utf8(explained.reply.body)
            .unwrap()
            .contains("Give /herald")
    );
    let other = String::from_utf8(form("latest"))
        .unwrap()
        .replace("%2Fherald", "%2Fother")
        .into_bytes();
    let ignored = post(&world, path, &signed_slack(&other), &other);
    assert_eq!((ignored.reply.status, ignored.job.is_none()), (200, true));
    let unread = b"text=latest";
    assert_eq!(
        post(&world, path, &signed_slack(unread), unread)
            .reply
            .status,
        400
    );
    assert_eq!(post(&world, path, &[], &latest).reply.status, 401);
}

#[test]
fn commands_for_a_destination_taking_none_over_http_are_not_served() {
    let world = world();
    for path in [
        "/interactions/quiet",
        "/interactions/gatewayed",
        "/interactions/socket",
        "/interactions/nobody",
        "/interactions/matrix",
    ] {
        assert_eq!(post(&world, path, &[], b"{}").reply.status, 404, "{path}");
    }
    let got = world.answer(&Inbound {
        method: "GET",
        path: "/interactions/discord",
        query: None,
        headers: &[],
        body: b"",
    });
    assert_eq!(got.reply.status, 405);
}

fn matrix(world: &World, method: &str, path: &str, token: &str, body: &[u8]) -> Answered {
    let header = format!("Bearer {token}");
    world.answer(&Inbound {
        method,
        path,
        query: None,
        headers: &[("Authorization", &header)],
        body,
    })
}

#[test]
fn a_homeserver_s_transaction_is_checked_admitted_once_and_its_commands_left_to_answer() {
    let world = world();
    let path = "/matrix/matrix/_matrix/app/v1/transactions/t1";
    let body = json!({ "events": [{
        "type": "m.room.message",
        "room_id": ROOM,
        "event_id": "$command",
        "sender": "@ann:matrix.example",
        "content": { "msgtype": "m.text", "body": "!herald latest" },
    }]})
    .to_string()
    .into_bytes();
    let pushed = matrix(&world, "PUT", path, "hs-token", &body);
    assert_eq!(
        (pushed.reply.status, pushed.reply.body.as_slice()),
        (200, b"{}".as_slice())
    );
    let Some(job) = pushed.job else {
        panic!("a command leaves its work");
    };
    assert!(
        matches!(&job, Job::Interact { asked, .. } if asked[0].0.place == Place::new(ROOM)),
        "{job:?}"
    );
    world.work(job);
    assert!(
        world
            .script
            .seen()
            .iter()
            .any(|line| line.contains("/send/m.room.message/")),
        "{:?}",
        world.script.seen()
    );
    let again = matrix(&world, "PUT", path, "hs-token", &body);
    assert_eq!((again.reply.status, again.job.is_none()), (200, true));
    let neighbour = matrix(
        &world,
        "PUT",
        "/matrix/neighbour/_matrix/app/v1/transactions/t1",
        "hs-token",
        &body,
    );
    assert!(
        neighbour.job.is_some(),
        "another application service numbers its transactions on its own"
    );
    let quiet = matrix(
        &world,
        "PUT",
        "/matrix/matrix/_matrix/app/v1/transactions/t2",
        "hs-token",
        br#"{"events": []}"#,
    );
    assert_eq!((quiet.reply.status, quiet.job.is_none()), (200, true));
    assert_eq!(
        matrix(
            &world,
            "PUT",
            "/matrix/matrix/_matrix/app/v1/transactions/t3",
            "hs-token",
            b"x"
        )
        .reply
        .status,
        400
    );
    assert_eq!(
        matrix(&world, "PUT", path, "wrong", &body).reply.status,
        401
    );
    let mut failing = world;
    failing.store.fault = Fault::Admits;
    assert_eq!(
        matrix(
            &failing,
            "PUT",
            "/matrix/matrix/_matrix/app/v1/transactions/t4",
            "hs-token",
            &body
        )
        .reply
        .status,
        503
    );
}

#[test]
fn a_homeserver_asking_after_users_rooms_and_its_ping_is_answered() {
    let world = world();
    let at = |path: &str| format!("/matrix/matrix/_matrix/app/v1/{path}");
    assert_eq!(
        matrix(
            &world,
            "GET",
            &at("users/@herald:matrix.example"),
            "hs-token",
            b""
        )
        .reply
        .status,
        200
    );
    assert_eq!(
        matrix(
            &world,
            "GET",
            &at("users/@ann:matrix.example"),
            "hs-token",
            b""
        )
        .reply
        .status,
        404
    );
    assert_eq!(
        matrix(
            &world,
            "GET",
            &at("rooms/#a:matrix.example"),
            "hs-token",
            b""
        )
        .reply
        .status,
        404
    );
    assert_eq!(
        matrix(&world, "POST", &at("ping"), "hs-token", b"{}")
            .reply
            .status,
        200
    );
    assert_eq!(
        matrix(&world, "DELETE", &at("ping"), "hs-token", b"")
            .reply
            .status,
        405
    );
    assert_eq!(
        matrix(&world, "GET", &at("thirdparty/protocol"), "hs-token", b"")
            .reply
            .status,
        404
    );
    assert_eq!(
        matrix(
            &world,
            "POST",
            "/matrix/slack/_matrix/app/v1/ping",
            "hs-token",
            b""
        )
        .reply
        .status,
        404
    );
}

#[test]
fn commands_left_to_answer_are_answered_and_sent() {
    let world = World::new(
        "",
        Script::answering(&[(
            &format!("/webhooks/{APPLICATION}/{TOKEN}/messages/@original"),
            200,
            "{}",
        )]),
    )
    .announced("gate", &["releases"]);
    let job = Job::Interact {
        destination: DestinationName::new("discord"),
        asked: vec![(discord(Command::Latest), discord_replying())],
    };
    let Worked::Interacted(interacted) = world.work(job.clone()) else {
        panic!("commands are answered");
    };
    assert_eq!(
        interacted,
        [Interacted {
            asked: discord(Command::Latest),
            answer: world.interact(&discord(Command::Latest)),
            sent: Ok(()),
        }]
    );
    assert_eq!(world.script.seen().len(), 1);
    let unsent = World::new("", Script::default()).announced("gate", &["releases"]);
    let Worked::Interacted(failed) = unsent.work(job) else {
        panic!("commands are answered");
    };
    assert!(
        failed[0]
            .sent
            .as_ref()
            .unwrap_err()
            .contains("nothing scripted")
    );
    unsent.serving(|serving| {
        pollster::block_on(unworked(
            serving,
            &Job::Interact {
                destination: DestinationName::new("discord"),
                asked: Vec::new(),
            },
        ));
    });
}
