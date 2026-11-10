use herald_core::{DestinationName, Subscription};
use herald_ports::{Secret, Subscriptions as _};
use herald_store_git_branch::GitBranch;

use super::activitypub::pkcs8;
use super::world::*;
use crate::*;

const FOLLOWED: &str = r#"version = 1
[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }
[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }
approve = "manual"
[sources.cli]
kind = "github"
repository = "cli/cli"
[[routes]]
id = "followed"
sources = ["cli"]
destination = "fedi"
actor = "gate"
template = "release"
[templates.release]
title = "{{ event.project }} {{ event.attributes.version }}"
body = "{{ event.body }}"
"#;

const ANA: &str = "https://social.example/users/ana";
const BO: &str = "https://social.example/users/bo";
const CY: &str = "https://social.example/users/cy";

fn followed() -> Setup {
    let mut setup = setup(FOLLOWED);
    setup
        .vars
        .insert("HERALD_ACTIVITYPUB_KEY".to_owned(), pkcs8());
    setup
}

fn kept(world: &World, follower: &str, pending: Option<&str>) {
    let store = GitBranch::new(
        "cli/cli",
        Secret::new("GITHUB_TOKEN", "github-token"),
        None,
        None,
    )
    .store(Connection::new(world.clone()))
    .sealed(herald_records::Seal::new(Secret::new(
        "HERALD_STORE_KEY",
        "store-key",
    )));
    pollster::block_on(store.subscribe(&Subscription {
        destination: DestinationName::new("fedi"),
        project: "gate".to_owned(),
        subscriber: follower.to_owned(),
        inbox: Some("https://social.example/inbox".to_owned()),
        pending: pending.map(ToOwned::to_owned),
    }))
    .unwrap();
}

const ACTOR: [&str; 4] = ["--destination", "fedi", "--actor", "gate"];

fn asked<'a>(command: &'a str, follower: Option<&'a str>) -> Vec<&'a str> {
    let mut args = vec!["followers", command];
    args.extend(ACTOR);
    if let Some(follower) = follower {
        args.extend(["--follower", follower]);
    }
    args
}

#[test]
fn an_actor_s_followers_are_listed_with_those_that_wait_last() {
    let setup = followed();
    let world = World::default();
    let none = herald(&setup, &world, &asked("list", None));
    assert_eq!(
        (none.code, none.out.as_str(), none.err.as_str()),
        (DONE, "No one follows gate, and no follow waits.\n", "")
    );
    kept(&world, BO, Some("https://social.example/f/2"));
    kept(&world, ANA, None);
    let listed = herald(&setup, &world, &asked("list", None));
    assert_eq!(
        (listed.code, listed.out.as_str()),
        (DONE, &*format!("follows: {ANA}\nwaits: {BO}\n"))
    );
    let unknown = herald(
        &setup,
        &world,
        &[
            "followers",
            "list",
            "--destination",
            "fedi",
            "--actor",
            "nobody",
        ],
    );
    assert_eq!(
        (unknown.code, unknown.err.as_str()),
        (
            STOPPED,
            "the destination \"fedi\" serves no actor \"nobody\"\n"
        )
    );
}

#[test]
fn a_waiting_follow_is_approved_or_rejected_and_answered_at_its_inbox() {
    let setup = followed();
    let world = World::default();
    kept(&world, ANA, Some("https://social.example/f/1"));
    kept(&world, BO, Some("https://social.example/f/2"));
    discord(&world, &[("/inbox", 202, ""), ("/inbox", 403, "")]);
    let approved = herald(&setup, &world, &asked("approve", Some(ANA)));
    assert_eq!(
        (approved.code, approved.out.as_str()),
        (DONE, &*format!("{ANA} follows gate now\n"))
    );
    let rejected = herald(&setup, &world, &asked("reject", Some(BO)));
    assert_eq!(rejected.code, STOPPED);
    assert_eq!(
        rejected.out,
        format!(
            "{BO} was turned away from gate\nIts inbox did not take the answer, which was not sent again: the inbox refused it with 403\n"
        )
    );
    let listed = herald(&setup, &world, &asked("list", None));
    assert_eq!(listed.out, format!("follows: {ANA}\n"));
    kept(&world, CY, Some("https://social.example/f/3"));
    discord(&world, &[("/inbox", 503, "")]);
    let unwell = herald(&setup, &world, &asked("approve", Some(CY)));
    assert_eq!(
        (unwell.code, unwell.out.as_str()),
        (
            STOPPED,
            &*format!(
                "{CY} follows gate now\nIts inbox did not take the answer, which was not sent again: it asked to wait 5s\n"
            )
        )
    );
    let again = herald(&setup, &world, &asked("approve", Some(ANA)));
    assert_eq!(
        (again.code, again.err.as_str()),
        (
            STOPPED,
            &*format!("no follow of \"gate\" by \"{ANA}\" waits for approval\n")
        )
    );
}
