use super::world::*;
use crate::*;

const RUN: &str =
    include_str!("../../../herald-sources-forge/fixtures/github/workflow-run-event.reference.json");

const RUNS: &str = r#"version = 1
[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.cli]
kind = "github"
repository = "cli/cli"
signals = ["run"]
[[routes]]
id = "builds"
sources = ["cli"]
destination = "discord"
channel = "1566200000000000100"
template = "run"
[templates.run]
title = "{{ event.title }}"
body = "{{ event.links.run }}"
"#;

fn ran(config: &str, payload: &str) -> Setup {
    let mut setup = setup(config);
    setup
        .vars
        .insert("GITHUB_EVENT_NAME".to_owned(), "workflow_run".to_owned());
    std::fs::write(setup.directory.path().join("event.json"), payload).unwrap();
    setup
}

#[test]
fn a_workflow_run_s_own_event_announces_the_run_once() {
    let setup = ran(RUNS, RUN);
    let world = World::default();
    world.state.lock().unwrap().discord.push((
        "/channels/1566200000000000100/messages".to_owned(),
        200,
        r#"{"id":"1"}"#.to_owned(),
    ));
    let first = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "builds: announced (post)\n", "")
    );
    let again = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "builds: already announced; nothing sent\n");
}

#[test]
fn a_workflow_run_the_source_does_not_announce_says_why_and_a_garbled_one_stops() {
    let world = World::default();
    let unran = herald(&ran(CONFIG, RUN), &world, &ANNOUNCE);
    assert_eq!(
        (unran.code, unran.out.as_str()),
        (
            DONE,
            "The run's event is workflow_run, and the source announces no runs of the repository; nothing to announce.\n"
        )
    );
    let unfinished = herald(
        &ran(
            RUNS,
            &RUN.replace("\"status\": \"completed\"", "\"status\": \"in_progress\""),
        ),
        &world,
        &ANNOUNCE,
    );
    assert_eq!(
        (unfinished.code, unfinished.out.as_str()),
        (
            DONE,
            "The run is not one the source's runs announce; nothing to announce.\n"
        )
    );
    let garbled = herald(&ran(RUNS, "{}"), &world, &ANNOUNCE);
    assert_eq!(garbled.code, STOPPED);
    assert!(
        garbled
            .err
            .starts_with("GitHub's workflow run event could not be read"),
        "{}",
        garbled.err
    );
    let mut lost = ran(RUNS, RUN);
    lost.vars.insert(
        "GITHUB_EVENT_PATH".to_owned(),
        "/nonexistent/event.json".to_owned(),
    );
    let unread = herald(&lost, &world, &ANNOUNCE);
    assert_eq!(unread.code, STOPPED);
    assert!(
        unread
            .err
            .contains("/nonexistent/event.json could not be read"),
        "{}",
        unread.err
    );
}
