use herald_ports::Method;

use super::world::*;
use crate::*;

#[test]
fn a_poll_remembers_what_it_first_finds_and_announces_what_is_new() {
    let setup = setup(CONFIG);
    let world = World::default();
    forge(&world, (200, &["v1"]));
    let first = herald(&setup, &world, &POLL);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (
            DONE,
            "cli: first poll found 1; 0 announced, as backfill asks\n",
            ""
        )
    );
    forge(&world, (200, &["v2", "v1"]));
    discord(&world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
    let later = herald(&setup, &world, &POLL);
    assert_eq!(
        (later.code, later.out.as_str()),
        (
            DONE,
            "cli: 2 found, 1 new\ncli v2: releases: announced (post)\n"
        )
    );
    forge(&world, (304, &[]));
    let unchanged = herald(&setup, &world, &POLL);
    assert_eq!(unchanged.out, "cli: unchanged since the last poll\n");
}

#[test]
fn a_poll_that_stops_says_what_the_next_poll_does() {
    let setup = setup(CONFIG);
    let world = World::default();
    forge(&world, (200, &["v1"]));
    world.state.lock().unwrap().failing = Some(Method::Put);
    let unkept = herald(&setup, &world, &POLL);
    assert_eq!(
        (unkept.code, unkept.out.as_str()),
        (
            STOPPED,
            "cli: first poll found 1; 0 announced, as backfill asks; the cursor stays where it was: the store could not be reached: GitHub answered 500. The next poll offers them again\n"
        )
    );
    world.state.lock().unwrap().failing = None;
    assert_eq!(herald(&setup, &world, &POLL).code, DONE);
    forge(&world, (200, &["v2", "v1"]));
    discord(
        &world,
        &[(
            "/channels/100/messages",
            403,
            r#"{"code":50013,"message":"Missing Permissions"}"#,
        )],
    );
    let refused = herald(&setup, &world, &POLL);
    assert_eq!(refused.code, STOPPED);
    assert!(
        refused
            .out
            .starts_with("cli: 2 found, 1 new; a route stopped, so the next poll offers them again\ncli v2: releases: stopped: Discord refused it with 403"),
        "{}",
        refused.out
    );
    world
        .state
        .lock()
        .unwrap()
        .forge
        .insert(REPOSITORY.to_owned(), (404, "{}".to_owned()));
    let unread = herald(&setup, &world, &POLL);
    assert_eq!(
        (unread.code, unread.out.as_str()),
        (
            STOPPED,
            "cli: stopped: GitHub's repository was answered with 404. The next poll asks again.\n"
        )
    );
}

#[test]
fn a_config_without_sources_has_nothing_to_poll() {
    let sourceless = "version = 1\n[store]\nkind = \"git-branch\"\n[destinations.discord]\nkind = \"discord\"\ntoken = { secret = \"HERALD_DISCORD_TOKEN\" }\n";
    let idle = herald(&setup(sourceless), &World::default(), &POLL);
    assert_eq!(
        (idle.code, idle.out.as_str(), idle.err.as_str()),
        (DONE, "The config names no source to poll.\n", "")
    );
}
