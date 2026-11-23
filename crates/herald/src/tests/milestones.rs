use herald_crypto::RingSigner;
use herald_ports::{Algorithm, Secret, Signer as _};
use serde_json::{Value, json};

use super::world::*;
use crate::{DONE, STOPPED};

const PUBLIC: &str = "/channels/1566200000000000100/messages";
const PRIVATE: &str = "/channels/1566200000000000200/messages";
const LISTING: &str = "/repos/cli/cli/milestones?state=open&per_page=30&page=1";
const SECRET: &str = "Phoenix";

const MILESTONES: &str = r#"version = 1
[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.cli]
kind = "github"
repository = "cli/cli"
signals = ["milestone"]
webhook = { secret = "HERALD_WEBHOOK" }
[[routes]]
id = "public"
sources = ["cli"]
destination = "discord"
channel = "1566200000000000100"
template = "milestone"
private = false
[[routes]]
id = "team"
sources = ["cli"]
destination = "discord"
channel = "1566200000000000200"
template = "milestone"
private = true
[templates.milestone]
title = "{{ event.title }}"
body = "{{ event.body }} {{ event.attributes.due }} {{ event.attributes.issues_open }}"
"#;

fn milestone(state: &str) -> Value {
    json!({
        "id": 1001, "number": 1, "title": format!("{SECRET} v2"), "description": format!("{SECRET} plans"),
        "state": state, "open_issues": 1, "closed_issues": 9,
        "created_at": "2026-11-18T08:00:00Z", "updated_at": "2026-11-18T10:00:00Z",
        "closed_at": (state == "closed").then_some("2026-11-18T11:00:00Z"),
        "due_on": "2026-12-01T08:00:00Z", "html_url": "https://github.com/cli/cli/milestone/1",
    })
}

fn hook(visibility: &str) -> String {
    json!({
        "action": "closed", "milestone": milestone("closed"),
        "repository": {
            "id": 212_613_049, "name": "cli", "full_name": "cli/cli", "html_url": "https://github.com/cli/cli",
            "private": visibility != "public", "visibility": visibility,
        },
    })
    .to_string()
}

fn sent(world: &World, channel: &str) -> usize {
    let seen = world.state.lock().unwrap().seen.clone();
    seen.iter().filter(|line| line.ends_with(channel)).count()
}

fn private_only(world: &World, run: &Run) {
    assert_eq!(sent(world, PRIVATE), 1, "{}{}", run.out, run.err);
    assert_eq!(sent(world, PUBLIC), 0, "{}", run.out);
    assert!(!run.out.contains(SECRET), "{}", run.out);
    assert!(!run.err.contains(SECRET), "{}", run.err);
}

fn closed_by_poll(config: &str, poll: &[&str]) -> (World, Run) {
    let setup = setup(config);
    let world = World::default();
    {
        let mut state = world.state.lock().unwrap();
        state.forge.insert(
            REPOSITORY.to_owned(),
            (
                200,
                r#"{"id":212613049,"name":"cli","visibility":"private"}"#.to_owned(),
            ),
        );
        state.forge.insert(
            LISTING.to_owned(),
            (200, json!([milestone("open")]).to_string()),
        );
    }
    let seeded = herald(&setup, &world, poll);
    assert_eq!(seeded.code, DONE, "{}{}", seeded.out, seeded.err);
    assert_eq!(sent(&world, PRIVATE) + sent(&world, PUBLIC), 0);
    {
        let mut state = world.state.lock().unwrap();
        state
            .forge
            .insert(LISTING.to_owned(), (200, "[]".to_owned()));
        state.forge.insert(
            "/repos/cli/cli/milestones/1".to_owned(),
            (200, milestone("closed").to_string()),
        );
    }
    discord(&world, &[(PRIVATE, 200, r#"{"id":"1"}"#)]);
    let closed = herald(&setup, &world, poll);
    assert_eq!(closed.code, DONE, "{}{}", closed.out, closed.err);
    (world, closed)
}

#[test]
fn a_private_repository_s_milestone_closed_by_poll_reaches_only_a_private_route() {
    let (world, closed) = closed_by_poll(MILESTONES, &POLL);
    assert!(
        closed.out.contains("cli (private): team: announced (post)"),
        "{}",
        closed.out
    );
    private_only(&world, &closed);
}

#[test]
fn poll_output_said_private_names_a_private_repository_s_milestone() {
    let configured = format!("{MILESTONES}[cli]\nprivate = true\n");
    for (config, poll) in [
        (MILESTONES, &["poll", "--private"][..]),
        (configured.as_str(), &POLL[..]),
    ] {
        let (world, closed) = closed_by_poll(config, poll);
        assert!(
            closed
                .out
                .contains(&format!("cli {SECRET} v2: team: announced (post)")),
            "{}",
            closed.out
        );
        assert_eq!(sent(&world, PRIVATE), 1, "{}", closed.out);
        assert_eq!(sent(&world, PUBLIC), 0, "{}", closed.out);
    }
}

fn lambda(world: &World, payload: &str) -> Run {
    let mut setup = setup(MILESTONES);
    for (name, value) in [
        ("AWS_LAMBDA_RUNTIME_API", "127.0.0.1:9001"),
        ("HERALD_WEBHOOK", "hook-secret"),
    ] {
        setup.vars.insert(name.to_owned(), value.to_owned());
    }
    let signature = on_tokio(RingSigner::default().sign(
        Algorithm::HmacSha256,
        &Secret::new("HERALD_WEBHOOK", "hook-secret"),
        payload.as_bytes(),
    ))
    .unwrap();
    world.state.lock().unwrap().invocations.push_back(json!({
        "version": "2.0",
        "requestContext": { "http": { "method": "POST" } },
        "rawPath": "/hooks/cli",
        "rawQueryString": "",
        "headers": {
            "x-hub-signature-256": format!("sha256={}", herald_core::hex(&signature)),
            "x-github-event": "milestone",
            "x-github-delivery": "d-1",
        },
        "body": payload,
        "isBase64Encoded": false,
    }));
    herald(&setup, world, &["lambda"])
}

#[test]
fn a_private_repository_s_milestone_closed_by_webhook_reaches_only_a_private_route() {
    let world = World::default();
    discord(&world, &[(PRIVATE, 200, r#"{"id":"1"}"#)]);
    let run = lambda(&world, &hook("private"));
    assert!(run.out.contains("team: announced (post)"), "{}", run.out);
    private_only(&world, &run);
    let world = World::default();
    discord(
        &world,
        &[
            (PUBLIC, 200, r#"{"id":"1"}"#),
            (PRIVATE, 200, r#"{"id":"2"}"#),
        ],
    );
    let run = lambda(&world, &hook("public"));
    assert!(run.out.contains("public: announced (post)"), "{}", run.out);
    assert_eq!(sent(&world, PUBLIC), 1, "{}", run.out);
}

#[test]
fn a_ci_run_s_milestone_event_of_a_private_repository_reaches_only_a_private_route() {
    let mut setup = setup(MILESTONES);
    setup
        .vars
        .insert("GITHUB_EVENT_NAME".to_owned(), "milestone".to_owned());
    std::fs::write(setup.directory.path().join("event.json"), hook("private")).unwrap();
    let world = World::default();
    discord(&world, &[(PRIVATE, 200, r#"{"id":"1"}"#)]);
    let run = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(run.code, DONE, "{}{}", run.out, run.err);
    private_only(&world, &run);
    let released = MILESTONES
        .replace("signals = [\"milestone\"]", "signals = [\"release\"]")
        .replace(
            " {{ event.attributes.due }} {{ event.attributes.issues_open }}",
            "",
        );
    let unsignalled = setup_with(&released, "milestone");
    let run = herald(&unsignalled, &World::default(), &ANNOUNCE);
    assert!(
        run.out.ends_with(
            "The run's event is milestone, and the source announces no milestones of the repository; nothing to announce.\n"
        ),
        "{}",
        run.out
    );
    let opened = setup_with(MILESTONES, "milestone");
    std::fs::write(
        opened.directory.path().join("event.json"),
        hook("private").replacen("\"closed\"", "\"created\"", 1),
    )
    .unwrap();
    let run = herald(&opened, &World::default(), &ANNOUNCE);
    assert!(
        run.out.ends_with(
            "The milestone is not one the source's milestones announce; nothing to announce.\n"
        ),
        "{}",
        run.out
    );
}

#[test]
fn a_ci_run_s_garbled_milestone_event_stops_the_announcement() {
    let garbled = setup_with(MILESTONES, "milestone");
    std::fs::write(garbled.directory.path().join("event.json"), "{}").unwrap();
    let run = herald(&garbled, &World::default(), &ANNOUNCE);
    assert_eq!(run.code, STOPPED);
    assert!(
        run.err
            .contains("GitHub's milestone event could not be read"),
        "{}",
        run.err
    );
}

fn setup_with(config: &str, event: &str) -> Setup {
    let mut setup = setup(config);
    setup
        .vars
        .insert("GITHUB_EVENT_NAME".to_owned(), event.to_owned());
    setup
}
