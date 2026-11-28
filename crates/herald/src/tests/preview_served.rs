//! `herald preview` of a route to one of herald's own feeds or a push to
//! a peer: the envelope it publishes, served rather than sent, its proof
//! by name alone, or why it publishes none (ADR-0095).

use super::preview::{heads, previewed, wired};
use super::world::*;
use crate::*;

/// [`super::preview::wired`]'s config with a feed of herald's own and a push to a peer by
/// the instance's key and by the run's ID token beside its actor.
pub(super) fn served() -> String {
    format!(
        r#"{}[federation]
domain = "herald.example"
keys = [{{ secret = "HERALD_FED_KEY" }}]
[peers.org]
url = "https://herald.peer.example"
keys = ["{}"]
[destinations.feed]
kind = "herald"
[destinations.mirror]
kind = "herald-push"
peer = "org"
inbox = "up"
[destinations.tokened]
kind = "herald-push"
peer = "org"
inbox = "up"
credential = "id-token"
[[routes]]
id = "fed"
sources = ["cli"]
destination = "feed"
[[routes]]
id = "pushed"
sources = ["cli"]
destination = "mirror"
[[routes]]
id = "tokened"
sources = ["cli"]
destination = "tokened"
"#,
        wired(),
        herald_peers::multibase::ed25519_public(&[5; 32])
    )
}

#[test]
fn preview_wire_shows_the_envelope_a_feed_serves_or_a_push_carries_its_proof_by_name_alone() {
    let text = previewed(&served(), &["--sample", "plain"]);
    assert_eq!(
        (text.code, heads(&text.out)[3..].to_vec()),
        (
            DONE,
            vec![
                "fed to feed, sample release (plain): it publishes each event as an envelope rather than a message; --format wire shows it",
                "pushed to mirror, sample release (plain): it publishes each event as an envelope rather than a message; --format wire shows it",
                "tokened to tokened, sample release (plain): it publishes each event as an envelope rather than a message; --format wire shows it",
            ]
        )
    );
    let wire = previewed(&served(), &["--sample", "plain", "--format", "wire"]);
    for said in [
        "  served, not sent: herald's feed lists this envelope, proved by «secret:HERALD_FED_KEY»:",
        "  served, not sent: herald pushes this envelope to the peer org's inbox, proved by «secret:HERALD_FED_KEY»:",
        "  served, not sent: herald pushes this envelope to the peer org's inbox, proved by the run's ID token:",
        "      \"type\": \"herald:Event\",",
    ] {
        assert!(
            wire.out.lines().any(|line| line == said),
            "{said}: {}",
            wire.out
        );
    }
    assert!(!wire.out.contains("\"proof\""), "{}", wire.out);
    let json = previewed(&served(), &["--sample", "plain", "--format", "json"]);
    let all: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    assert_eq!(
        (
            &all[3]["title"],
            &all[3]["served"]["by"],
            &all[3]["served"]["name"],
            &all[3]["served"]["proof"],
            &all[3]["served"]["document"]["type"],
            &all[5]["served"]["name"],
        ),
        (
            &serde_json::Value::Null,
            &serde_json::json!("feed"),
            &serde_json::Value::Null,
            &serde_json::json!("«secret:HERALD_FED_KEY»"),
            &serde_json::json!("herald:Event"),
            &serde_json::json!("org"),
        )
    );
}

#[test]
fn preview_says_why_a_feed_or_a_push_to_a_peer_taking_no_private_events_publishes_none_of_a_private_event()
 {
    let setup = setup(&served());
    let file = setup.directory.path().join("event.json");
    let private = EVENT
        .replace("\"private\": false", "\"private\": true")
        .replace("\"visibility\": \"public\"", "\"visibility\": \"private\"");
    std::fs::write(&file, private).unwrap();
    let file = file.display().to_string();
    let asked = |format: &'static str| {
        herald(
            &setup,
            &World::default(),
            &["preview", "--event", &file, "--private", "--format", format],
        )
    };
    let wire = asked("wire");
    for (route, why) in [
        ("fed to feed", "so it enters none"),
        ("pushed to mirror", "so it is pushed none"),
        ("tokened to tokened", "so it is pushed none"),
    ] {
        assert!(
            wire.out
                .lines()
                .any(|line| line.starts_with(route) && line.ends_with(why)),
            "{route}: {}",
            wire.out
        );
    }
    let json: serde_json::Value = serde_json::from_str(&asked("json").out).unwrap();
    let fed = json
        .as_array()
        .unwrap()
        .iter()
        .find(|shown| shown["route"] == "fed")
        .unwrap();
    assert_eq!(fed["served"], serde_json::Value::Null);
    assert!(
        fed["unsent"]
            .as_str()
            .unwrap()
            .ends_with("so it enters none"),
        "{fed}"
    );
}
