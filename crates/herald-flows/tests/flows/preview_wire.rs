use herald_core::{Announcement, Pinning, RouteId, Step};
use herald_flows::{DRY_ID, Proof, Publisher, UNENVELOPED, UNREACHED, Wire, preview, wired};
use herald_ports::{Http as _, Method, Request, SecretText};

use crate::harness::*;

fn announced(config: &herald_config::Config) -> Announcement {
    preview(config, &event("v1"), None)
        .remove(0)
        .rendered
        .unwrap()
}

fn sent(config: &herald_config::Config, route: &str) -> Wire {
    wired_with(config, route, &event("v1"))
}

fn wired_with(config: &herald_config::Config, route: &str, event: &herald_core::Event) -> Wire {
    let announcement = announced(&crate::harness::config(""));
    pollster::block_on(wired(config, &RouteId::new(route), (&announcement, event)))
}

fn shown(requests: &[Request]) -> Vec<String> {
    requests
        .iter()
        .map(|request| request.with_body().to_string())
        .collect()
}

#[test]
fn a_first_post_sends_each_step_its_destination_writes_with_each_secret_by_its_name() {
    let Wire::Sent(steps) = sent(&config(""), "releases") else {
        panic!("the route sends nothing");
    };
    assert_eq!(
        steps
            .iter()
            .map(|(step, _)| step.clone())
            .collect::<Vec<_>>(),
        all()
    );
    let channel = "https://discord.com/api/v10/channels/1566200000000000100/messages";
    let posted = &steps[0].1;
    assert_eq!(posted.len(), 1);
    assert_eq!(
        (posted[0].method, posted[0].url.redacted()),
        (Method::Post, channel.to_owned())
    );
    let authorised = posted[0]
        .headers
        .iter()
        .find(|(name, _)| name == "Authorization")
        .map(|(_, value)| (value.redacted(), value.expose()));
    assert_eq!(
        authorised,
        Some((
            "Bot «secret:HERALD_DISCORD_TOKEN»".to_owned(),
            "Bot ".to_owned()
        ))
    );
    assert!(
        String::from_utf8_lossy(&posted[0].body).contains("📦 php-mutation-gate v1"),
        "{posted:?}"
    );
    assert_eq!(
        steps[1].1[0].url.redacted(),
        format!("{channel}/{DRY_ID}/crosspost")
    );
    assert_eq!(
        steps[2].1[0].url.redacted(),
        format!("{channel}/{DRY_ID}/threads")
    );
    assert_eq!(
        shown(&steps[3].1),
        [format!(
            "PUT {channel}/pins/{DRY_ID}\nUser-Agent: DiscordBot (https://github.com/nightworksio/herald, {})\nAuthorization: Bot «secret:HERALD_DISCORD_TOKEN»",
            herald_core::itself::VERSION
        )]
    );
    assert_eq!(
        steps[4]
            .1
            .iter()
            .map(|request| (request.method, request.url.redacted()))
            .collect::<Vec<_>>(),
        [
            (
                Method::Get,
                "https://discord.com/api/v10/users/@me".to_owned()
            ),
            (Method::Get, format!("{channel}/pins?limit=50")),
        ]
    );
}

#[test]
fn a_route_keeping_its_pin_in_the_store_unpins_nothing_on_its_first_post() {
    let mut config = config("");
    config.routes[0].route.delivery.pin = Some(Pinning::Store);
    let Wire::Sent(steps) = sent(&config, "releases") else {
        panic!("the route sends nothing");
    };
    let unpinned = steps
        .iter()
        .find(|(step, _)| *step == Step::Unpin)
        .map(|(_, requests)| requests.len());
    assert_eq!(unpinned, Some(0));
    let pinned = steps
        .iter()
        .find(|(step, _)| *step == Step::Pin)
        .map(|(_, requests)| requests.len());
    assert_eq!(pinned, Some(1));
}

/// The document `wire` serves, read as JSON, with who publishes it and
/// what proves it.
fn served(wire: Wire) -> (Publisher, Proof, serde_json::Value) {
    let Wire::Served(served) = wire else {
        panic!("{wire:?} serves nothing");
    };
    let document = serde_json::from_str(&served.document).unwrap();
    (served.by, served.proof, document)
}

#[test]
fn a_route_herald_serves_or_pushes_shows_the_document_it_publishes_its_proof_by_name_alone() {
    let (by, proof, note) = served(sent(&crate::activitypub::activitypub(), "followed"));
    assert_eq!(by, Publisher::Actor("php-mutation-gate".to_owned()));
    assert_eq!(proof, Proof::Key("HERALD_ACTIVITYPUB_KEY".to_owned()));
    assert_eq!(note["type"], "Note");
    let (by, proof, envelope) = served(sent(&crate::feeds::config("HERALD_KEY", ""), "fed"));
    assert_eq!(
        (by, proof.named()),
        (Publisher::Feed, "«secret:HERALD_KEY»".to_owned())
    );
    assert_eq!(
        (
            &envelope["type"],
            &envelope["project"],
            envelope.get("proof")
        ),
        (
            &serde_json::json!("herald:Event"),
            &serde_json::json!("php-mutation-gate"),
            None
        )
    );
    let (by, proof, pushed) = served(sent(&crate::pushing::config(""), "pushed"));
    assert_eq!(
        (by, proof),
        (
            Publisher::Peer("receiver".to_owned()),
            Proof::Key("PUSH_KEY".to_owned())
        )
    );
    assert_eq!(
        (&pushed["visibility"], pushed.get("proof")),
        (&serde_json::json!("public"), None)
    );
    let tokened = crate::pushing::configured("", "credential = \"id-token\"");
    let (_, proof, _) = served(sent(&tokened, "pushed"));
    assert_eq!(proof.named(), "the run's ID token");
    assert_eq!(sent(&config(""), "elsewhere"), Wire::Unsent(UNREACHED));
}

#[test]
fn a_private_event_is_served_by_no_feed_and_pushed_only_to_a_peer_taking_private_events() {
    let private = event("v1").seen_as(herald_core::Visibility::Private);
    let Wire::Unsent(fed) = wired_with(&crate::feeds::config("HERALD_KEY", ""), "fed", &private)
    else {
        panic!("a feed served a private event");
    };
    assert!(fed.contains("enters none"), "{fed}");
    let Wire::Unsent(unshared) = wired_with(&crate::pushing::config(""), "pushed", &private) else {
        panic!("a push took a private event");
    };
    assert!(unshared.contains("pushed none"), "{unshared}");
    let (_, _, pushed) = served(wired_with(
        &crate::pushing::config("private = true"),
        "pushed",
        &private,
    ));
    assert_eq!(pushed["visibility"], "private");
    let long = herald_core::Event::normalise(herald_core::Draft {
        identity: "github.com/1/v1".to_owned(),
        kind: herald_core::Kind::RELEASE,
        project: "p".repeat(2048),
        attributes: herald_core::versioned("v1".to_owned(), false),
        title: "t".to_owned(),
        body: String::new(),
        links: std::collections::BTreeMap::new(),
        occurred: event("v1").occurred(),
    })
    .unwrap();
    assert_eq!(
        wired_with(&crate::feeds::config("HERALD_KEY", ""), "fed", &long),
        Wire::Unsent(UNENVELOPED)
    );
}

#[test]
fn a_recording_keeps_each_request_until_taken_and_answers_each_with_the_dry_id_as_id_and_token() {
    let recording = herald_flows::Recording::default();
    let request = Request::new(Method::Get, SecretText::plain("https://example.org"));
    let answer = pollster::block_on(recording.send(request.clone())).unwrap();
    assert_eq!(
        (answer.status, answer.body),
        (
            200,
            format!(r#"{{"id":"{DRY_ID}","access_token":"{DRY_ID}","expires_in":3600}}"#)
                .into_bytes()
        )
    );
    assert_eq!(recording.taken(), [request]);
    assert_eq!(recording.taken(), []);
}
