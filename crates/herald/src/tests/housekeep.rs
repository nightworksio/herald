use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_core::Chore;
use herald_ports::Method;

use super::world::*;
use crate::*;

#[test]
fn housekeeping_does_each_chore_that_fell_due_and_says_so() {
    let setup = setup(CONFIG);
    let world = World::default();
    let idle = herald(&setup, &world, &["housekeep"]);
    assert_eq!(
        (idle.code, idle.out.as_str(), idle.err.as_str()),
        (DONE, "No chore is due.\n", "")
    );
    scheduled(
        &world,
        &[
            ("releases", "7", Chore::Archive, OVERDUE),
            ("gone", "8", Chore::Lock, OVERDUE),
            ("releases", "9", Chore::Lock, DISTANT),
        ],
    );
    discord(&world, &[("/channels/7", 200, "{}")]);
    let kept = herald(&setup, &world, &["housekeep"]);
    assert_eq!(
        (kept.code, kept.out.as_str()),
        (
            DONE,
            "releases: thread 7 archived\ngone: thread 8 not locked, and not tried again: the route is no longer in the config\n"
        )
    );
    let again = herald(&setup, &world, &["housekeep"]);
    assert_eq!(again.out, "No chore is due.\n");
}

#[test]
fn housekeeping_that_must_wait_or_cannot_read_its_store_stops() {
    let setup = setup(CONFIG);
    let world = World::default();
    scheduled(
        &world,
        &[
            ("releases", "7", Chore::Archive, OVERDUE),
            ("releases", "8", Chore::Archive, OVERDUE),
        ],
    );
    discord(&world, &[("/channels/7", 429, r#"{"retry_after":2}"#)]);
    let waiting = herald(&setup, &world, &["housekeep"]);
    assert_eq!(waiting.code, STOPPED);
    assert_eq!(
        waiting.out.lines().collect::<Vec<_>>(),
        [
            "releases: thread 7 not archived yet: the destination asked to wait 2s. The next run tries again.",
            "releases: thread 8 not archived yet: the destination asked to wait 2s. The next run tries again.",
        ]
    );
    world.state.lock().unwrap().failing = Some(Method::Get);
    let unread = herald(&setup, &world, &["housekeep"]);
    assert_eq!(
        (unread.code, unread.out.as_str(), unread.err.as_str()),
        (
            STOPPED,
            "",
            "the store could not be reached: GitHub answered 500\n"
        )
    );
    let mut outside = setup;
    outside.vars.remove("GITHUB_REPOSITORY");
    let unset = herald(&outside, &world, &["housekeep"]);
    assert_eq!(
        unset.err,
        "GITHUB_REPOSITORY is not set; name the store's repository, or run herald in the repository's CI\n"
    );
}

#[test]
fn housekeeping_says_which_records_it_skipped_as_corrupt() {
    let setup = setup(CONFIG);
    let world = World::default();
    scheduled(&world, &[("releases", "9", Chore::Lock, DISTANT)]);
    world.state.lock().unwrap().files.insert(
        "schedules/bad.json".to_owned(),
        ("sha-bad".to_owned(), STANDARD.encode("not json")),
    );
    let kept = herald(&setup, &world, &["housekeep"]);
    assert_eq!(kept.code, DONE, "{}", kept.err);
    assert_eq!(
        kept.out.lines().collect::<Vec<_>>(),
        [
            "No chore is due.",
            "the record schedules/bad.json could not be read and was skipped; fix or remove it",
        ]
    );
}

#[test]
fn housekeeping_forgets_the_keys_the_store_keeps_once_they_are_due_a_reread() {
    let setup = setup(&format!("{CONFIG}[server.keys]\ncache = \"store\"\n"));
    let world = World::default();
    let store = kept(&world);
    let read = |fetched: &str| herald_ports::KnownKey {
        owner: "https://social.example/users/ann".to_owned(),
        pem: "key".to_owned(),
        fetched: fetched.parse().unwrap(),
    };
    pollster::block_on(store.know("stale", &read(OVERDUE))).unwrap();
    pollster::block_on(store.know("fresh", &read(DISTANT))).unwrap();
    let kept = herald(&setup, &world, &["housekeep"]);
    assert_eq!(kept.code, DONE, "{}", kept.err);
    assert_eq!(pollster::block_on(store.known("stale")).unwrap(), None);
    assert_eq!(
        pollster::block_on(store.known("fresh")).unwrap(),
        Some(read(DISTANT))
    );
}
