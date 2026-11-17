use super::world::*;
use crate::*;

/// The release the run's event announces.
const IDENTITY: &str = "github.com/212613049/v2.103.3";

fn announced(world: &World, setup: &Setup) {
    discord(
        world,
        &[(
            "/channels/1566200000000000100/messages",
            200,
            r#"{"id":"1"}"#,
        )],
    );
    assert_eq!(herald(setup, world, &ANNOUNCE).code, DONE);
}

#[test]
fn a_release_s_posts_are_withdrawn_from_a_route_or_every_route_and_a_dry_run_sends_nothing() {
    let setup = setup(CONFIG);
    let world = World::default();
    announced(&world, &setup);
    let planned = herald(
        &setup,
        &world,
        &["withdraw", "--every-route", "--dry-run", IDENTITY],
    );
    assert_eq!(
        (planned.code, planned.out.as_str()),
        (
            DONE,
            &*format!("releases: would withdraw {IDENTITY} (deletion of the post)\n")
        )
    );
    discord(
        &world,
        &[("/channels/1566200000000000100/messages/1", 204, "")],
    );
    let withdrawn = herald(&setup, &world, &["withdraw", "releases", IDENTITY]);
    assert_eq!(
        (
            withdrawn.code,
            withdrawn.out.as_str(),
            withdrawn.err.as_str()
        ),
        (
            DONE,
            &*format!("releases: withdrew {IDENTITY} (deletion of the post)\n"),
            ""
        )
    );
    let again = herald(&setup, &world, &["withdraw", "--every-route", IDENTITY]);
    assert_eq!(
        again.out,
        format!(
            "withdrawing {IDENTITY}: releases: skipped, as the route announced nothing of the release; nothing sent\n"
        )
    );
    let unplanned = herald(
        &setup,
        &world,
        &["withdraw", "--dry-run", "releases", IDENTITY],
    );
    assert_eq!(
        unplanned.out,
        format!("releases: announced nothing of {IDENTITY}\n")
    );
    let unknown = herald(&setup, &world, &["withdraw", "nope", IDENTITY]);
    assert_eq!(
        (unknown.code, unknown.out.as_str()),
        (STOPPED, "The config names no such route.\n")
    );
}

#[test]
fn a_withdrawal_names_a_route_and_an_identity_or_an_identity_for_every_route() {
    let setup = setup(CONFIG);
    let world = World::default();
    let alone = herald(&setup, &world, &["withdraw", IDENTITY]);
    assert_eq!(
        (alone.code, alone.err.as_str()),
        (
            STOPPED,
            "name the route and the release's identity, or give --every-route\n"
        )
    );
    let both = herald(
        &setup,
        &world,
        &["withdraw", "--every-route", "releases", IDENTITY],
    );
    assert_eq!(
        (both.code, both.err.as_str()),
        (
            STOPPED,
            "--every-route takes the release's identity alone\n"
        )
    );
}

#[test]
fn a_withdrawal_that_stops_or_a_plan_that_cannot_be_read_says_so() {
    let setup = setup(CONFIG);
    let world = World::default();
    announced(&world, &setup);
    discord(
        &world,
        &[(
            "/channels/1566200000000000100/messages/1",
            403,
            r#"{"code":50013,"message":"Missing Permissions"}"#,
        )],
    );
    let stopped = herald(&setup, &world, &["withdraw", "releases", IDENTITY]);
    assert_eq!(stopped.code, STOPPED);
    assert!(
        stopped
            .out
            .starts_with(&format!("withdrawing {IDENTITY}: releases: stopped: ")),
        "{}",
        stopped.out
    );
    world.state.lock().unwrap().failing = Some(herald_ports::Method::Get);
    let unread = herald(
        &setup,
        &world,
        &["withdraw", "--dry-run", "releases", IDENTITY],
    );
    assert_eq!(unread.code, STOPPED);
    assert!(
        unread.out.starts_with("nothing was read: "),
        "{} {}",
        unread.out,
        unread.err
    );
}
