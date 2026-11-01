use super::world::*;
use crate::*;

#[test]
fn a_release_kept_only_in_its_post_is_announced_once_and_poll_stops_without_cursors() {
    let mut setup = setup(&CONFIG.replace("kind = \"git-branch\"", MARKERS));
    setup.vars.remove("GITHUB_TOKEN");
    let world = World::default();
    discord(
        &world,
        &[
            ("/users/@me", 200, r#"{"id":"99"}"#),
            ("/channels/100/messages?limit=100", 200, "[]"),
            ("/channels/100/messages", 200, r#"{"id":"1"}"#),
        ],
    );
    let announced = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (announced.code, announced.out.as_str()),
        (DONE, "releases: announced (post)\n")
    );
    setup
        .vars
        .insert("GITHUB_ACTIONS".to_owned(), "true".to_owned());
    let polled = herald(&setup, &world, &POLL);
    assert_eq!(
        (polled.code, polled.out.as_str()),
        (
            STOPPED,
            "::add-mask::discord-token\ncli: stopped: this store keeps no cursors; choose a store that does. The next poll asks again.\n"
        )
    );
    let housekept = herald(&setup, &world, &["housekeep"]);
    assert!(
        housekept.out.ends_with("No chore is due.\n"),
        "{}",
        housekept.out
    );
}

#[test]
fn a_release_is_announced_once_through_the_actions_cache() {
    let mut setup = setup(&CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"actions-cache\"\nprefix = \"run-7\"",
    ));
    let world = World::default();
    discord(&world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
    let unset = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        unset.err,
        "ACTIONS_RESULTS_URL is not set; this command runs in GitHub Actions\n"
    );
    setup.vars.insert(
        "ACTIONS_RESULTS_URL".to_owned(),
        "https://results.example".to_owned(),
    );
    let tokenless = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        tokenless.err,
        "ACTIONS_RUNTIME_TOKEN is not set; the actions-cache store runs through herald's Action\n"
    );
    setup
        .vars
        .insert("ACTIONS_RUNTIME_TOKEN".to_owned(), "runtime".to_owned());
    let first = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    let again = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
    assert!(
        world
            .state
            .lock()
            .unwrap()
            .cache
            .keys()
            .all(|key| key.starts_with("run-7-"))
    );
}

#[test]
fn a_release_is_announced_once_through_the_repository_s_variables() {
    let config = CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"repo-variables\"\ntoken = { secret = \"HERALD_STATE_TOKEN\" }",
    );
    let mut setup = setup(&config);
    let world = World::default();
    discord(&world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
    let unset = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        unset.err,
        "HERALD_STATE_TOKEN is not set; the repo-variables store needs a token with the repository's Variables permission\n"
    );
    setup
        .vars
        .insert("HERALD_STATE_TOKEN".to_owned(), "variables".to_owned());
    let first = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    let again = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
    assert!(
        world
            .state
            .lock()
            .unwrap()
            .variables
            .keys()
            .all(|name| name.starts_with("HERALD_"))
    );
}
