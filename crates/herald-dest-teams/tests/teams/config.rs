//! A Teams destination's table and route, read as the config reads them,
//! and what the doctor's probe makes of its webhook.

use herald_core::{Chore, Delivery, Features, MentionStyle, Pins, Sent, Threads};
use herald_dest_teams::{DestinationOptions, KIND, RouteOptions, UNCHECKED, profile};
use herald_kinds::{Audience, Checking, DestinationConfig, Placed, Placement, Probe, Recorded};
use herald_ports::{Base, Response, Secret};
use herald_testing::{Fixture, Replay};
use serde_json::json;

fn options() -> DestinationOptions {
    serde_json::from_value(json!({ "webhook": { "secret": "HERALD_TEAMS_WEBHOOK" } })).unwrap()
}

fn placed() -> Placed {
    Placed::new(RouteOptions::default())
}

fn probed(held: &str) -> Vec<Probe> {
    let reached = options()
        .reach(&placed(), "", Secret::new("HERALD_TEAMS_WEBHOOK", held))
        .unwrap();
    let replay = Replay::of(Fixture::default());
    let probes = pollster::block_on(reached.probe(&replay, &Delivery::default(), Recorded::Kept));
    replay.finish().unwrap();
    probes
}

#[test]
fn a_teams_table_names_its_webhook_and_posts_only() {
    let options = options();
    assert_eq!(options.kind(), KIND);
    assert_eq!(
        options.credential().map(|secret| secret.secret.as_str()),
        Some("HERALD_TEAMS_WEBHOOK")
    );
    assert_eq!(options.secret_refs().len(), 1);
    assert_eq!(options.checking(&placed()), Checking::Unchecked(UNCHECKED));
    assert_eq!(options.public(&placed(), &Delivery::default()), None);
    assert_eq!(options.profile(&placed()), Some((profile(), None)));
    assert_eq!(
        profile().features,
        Features {
            threads: Threads::None,
            crosspost: false,
            pins: Pins::None,
            reads_back: false,
            edits: false,
            mentions: MentionStyle::None,
            interactions: false,
        }
    );
    assert!(serde_json::from_value::<DestinationOptions>(json!({})).is_err());
    assert!(
        serde_json::from_value::<DestinationOptions>(
            json!({ "webhook": { "secret": "W" }, "token": { "secret": "T" } })
        )
        .is_err()
    );
}

#[test]
fn a_route_says_nothing_of_teams_and_reads_back_as_written() {
    let route = RouteOptions::default();
    assert_eq!(route.place(), None);
    let written = route.written().unwrap();
    assert_eq!(written, "{}");
    assert!(options().recalled(&written).is_some());
    assert!(options().recalled("{\"channel\":\"x\"}").is_none());
    assert!(serde_json::from_value::<RouteOptions>(json!({ "channel": "x" })).is_err());
}

#[test]
fn the_doctor_is_told_a_webhook_cannot_be_asked_without_posting() {
    let url = ["https://", "hook.example/x?sig=", "s3"].concat();
    assert_eq!(
        probed(&url),
        [Probe::Said("a Workflows webhook cannot be asked whether it takes its URL without posting to it; herald doctor --online --post posts one test message there".to_owned())]
    );
    let reached = options()
        .reach(&placed(), "", Secret::new("HERALD_TEAMS_WEBHOOK", url))
        .unwrap();
    assert_eq!(reached.base(), Base::At("https://hook.example"));
    assert!(!reached.keeps_edits());
}

#[test]
fn the_doctor_is_told_which_secret_holds_no_https_url_and_never_what_it_holds() {
    let held = "hook.example/x?sig=s4";
    let probes = probed(held);
    assert_eq!(
        probes,
        [Probe::Credential("the secret HERALD_TEAMS_WEBHOOK holds no HTTPS URL; give it the URL the workflow's trigger, \"When a Teams webhook request is received\", shows".to_owned())]
    );
    assert!(!format!("{probes:?}").contains(held));
}

#[test]
fn a_webhook_reads_nothing_back_cannot_tell_who_reads_it_and_keeps_every_record_it_sent() {
    let reached = options()
        .reach(
            &placed(),
            "",
            Secret::new("HERALD_TEAMS_WEBHOOK", "https://hook.example/x"),
        )
        .unwrap();
    let replay = Replay::of(Fixture::default());
    assert_eq!(
        pollster::block_on(reached.audience(&replay)),
        Audience::Unread(UNCHECKED.to_owned())
    );
    assert_eq!(
        pollster::block_on(reached.housekeep(&replay, Chore::Archive, "t")).unwrap(),
        Sent::Refused("Microsoft Teams cannot archive or lock a thread".to_owned())
    );
    drop(reached.posts(&replay));
    let refusal = Response {
        status: 400,
        headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
        body: br#"{"error":{"code":"x","message":"y"}}"#.to_vec(),
    };
    assert!(!reached.declines(&refusal));
    replay.finish().unwrap();
}
