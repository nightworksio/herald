use std::collections::BTreeMap;

use herald_core::RouteId;

use super::world::*;
use crate::output::escaped;
use crate::*;

#[test]
fn a_published_release_is_announced_once_and_kept_on_the_state_branch() {
    let setup = setup(CONFIG);
    let world = World::default();
    world.state.lock().unwrap().discord.push((
        "/channels/100/messages".to_owned(),
        200,
        r#"{"id":"1"}"#.to_owned(),
    ));
    let first = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    assert!(world.state.lock().unwrap().branch);
    let again = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
}

#[test]
fn a_refusal_stops_the_run_and_says_what_remains() {
    let setup = setup(CONFIG);
    let world = World::default();
    world.state.lock().unwrap().discord.push((
        "/channels/100/messages".to_owned(),
        403,
        r#"{"code":50013,"message":"Missing Permissions"}"#.to_owned(),
    ));
    let stopped = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(stopped.code, STOPPED);
    assert_eq!(
        stopped.out,
        "releases: stopped: Discord refused it with 403: 50013 Missing Permissions. Landed: nothing. The next run sends what remains: post.\n"
    );
}

#[test]
fn an_event_that_is_not_a_published_release_announces_nothing() {
    let mut setup = setup(CONFIG);
    let world = World::default();
    setup
        .vars
        .insert("GITHUB_EVENT_NAME".to_owned(), "push".to_owned());
    let push = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (push.code, push.out.as_str()),
        (
            DONE,
            "The run's event is push, not a release; nothing to announce.\n"
        )
    );
    setup
        .vars
        .insert("GITHUB_EVENT_NAME".to_owned(), "release".to_owned());
    std::fs::write(
        setup.directory.path().join("event.json"),
        EVENT.replace("\"published\"", "\"created\""),
    )
    .unwrap();
    let created = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(
        created.out,
        "The release is not published, or is a draft; nothing to announce.\n"
    );
    std::fs::write(setup.directory.path().join("event.json"), "{}").unwrap();
    let garbled = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(garbled.code, STOPPED);
    assert!(
        garbled
            .err
            .starts_with("GitHub's release event could not be read"),
        "{}",
        garbled.err
    );
}

#[test]
fn a_run_that_cannot_start_says_why() {
    let world = World::default();
    let cases: [(&str, Option<&str>, &str, &str); 7] = [
        (
            "GITHUB_EVENT_NAME",
            None,
            CONFIG,
            "GITHUB_EVENT_NAME is not set; herald reads it from the CI run",
        ),
        (
            "GITHUB_EVENT_PATH",
            Some("/nonexistent/event.json"),
            CONFIG,
            "/nonexistent/event.json could not be read:",
        ),
        (
            "GITHUB_REPOSITORY",
            Some("other/repo"),
            CONFIG,
            "no source in the config names other/repo",
        ),
        (
            "GITHUB_TOKEN",
            None,
            CONFIG,
            "GITHUB_TOKEN is not set; the git-branch store needs a token that may push",
        ),
        ("HERALD_DISCORD_TOKEN", None, CONFIG, "HERALD_DISCORD_TOKEN"),
        (
            "GITHUB_RUN_ID",
            None,
            &CONFIG.replace("kind = \"git-branch\"", "kind = \"sqlite\""),
            "this herald keeps records in git-branch, markers, actions-cache or repo-variables; set [store] kind to one of them",
        ),
        (
            "GITHUB_RUN_ID",
            None,
            &CONFIG.replace(
                "kind = \"git-branch\"",
                "kind = \"git-branch\"\napi = \"https://unreachable.example\"",
            ),
            "https://unreachable.example cannot be reached",
        ),
    ];
    for (name, value, config, message) in cases {
        let mut setup = setup(config);
        match value {
            Some(value) => setup.vars.insert(name.to_owned(), value.to_owned()),
            None => setup.vars.remove(name),
        };
        let failed = herald(&setup, &world, &ANNOUNCE);
        assert_eq!(failed.code, STOPPED, "{name}: {}", failed.err);
        assert!(failed.err.contains(message), "{name}: {}", failed.err);
    }
}

#[test]
fn a_destination_that_cannot_be_reached_or_a_run_without_ids_still_answers() {
    let config = CONFIG.replace(
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }\napi = \"https://unreachable.example/api\"",
    );
    let failed = herald(&setup(&config), &World::default(), &ANNOUNCE);
    assert!(
        failed
            .err
            .contains("https://unreachable.example/api cannot be reached"),
        "{}",
        failed.err
    );
    let mut local = setup(CONFIG);
    local.vars.remove("GITHUB_RUN_ID");
    let world = World::default();
    world.state.lock().unwrap().discord.push((
        "/channels/100/messages".to_owned(),
        200,
        r#"{"id":"1"}"#.to_owned(),
    ));
    assert_eq!(herald(&local, &world, &ANNOUNCE).code, DONE);
}

#[test]
fn a_config_that_cannot_be_read_or_has_mistakes_is_reported() {
    let world = World::default();
    let mistaken = herald(&setup("version = 2\n"), &world, &["doctor"]);
    assert_eq!(mistaken.code, MISTAKEN);
    assert!(
        mistaken
            .err
            .contains("version: this herald reads version 1"),
        "{}",
        mistaken.err
    );
    let args = [
        "herald".to_owned(),
        "--config".to_owned(),
        "/nonexistent/herald.toml".to_owned(),
        "doctor".to_owned(),
    ];
    let mut err = Vec::new();
    let missing = pollster::block_on(crate::run(
        &args,
        &BTreeMap::new(),
        &|_| Ok(Connection::new(world.clone())),
        &mut Vec::new(),
        &mut err,
    ));
    assert_eq!(missing, MISTAKEN);
    assert!(
        String::from_utf8(err)
            .unwrap()
            .starts_with("/nonexistent/herald.toml could not be read")
    );
}

#[test]
fn a_template_file_that_cannot_be_read_and_a_store_token_of_its_own_are_named() {
    let world = World::default();
    let missing = herald(
        &setup(&CONFIG.replace("body.md", "missing.md")),
        &world,
        &["doctor"],
    );
    assert_eq!(missing.code, MISTAKEN);
    assert!(
        missing.err.contains("missing.md could not be read"),
        "{}",
        missing.err
    );
    let tokened = CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"git-branch\"\ntoken = { secret = \"STATE_TOKEN\" }",
    );
    let unset = herald(&setup(&tokened), &world, &ANNOUNCE);
    assert!(
        unset.err.contains("STATE_TOKEN is not set"),
        "{}",
        unset.err
    );
}

#[test]
fn usage_mistakes_and_help_answer_as_clap_does() {
    let world = World::default();
    let setup = setup(CONFIG);
    let unknown = herald(&setup, &world, &["fly"]);
    assert_eq!(unknown.code, 2);
    assert!(
        unknown.err.contains("unrecognized subcommand"),
        "{}",
        unknown.err
    );
    let help = herald(&setup, &world, &["--help"]);
    assert_eq!(help.code, 0);
    assert!(help.out.contains("Announce what happened"), "{}", help.out);
}

#[test]
fn every_outcome_and_step_reads_as_a_line() {
    let announced = |outcome| Announced {
        route: RouteId::new("r"),
        outcome,
    };
    let until: jiff::Timestamp = NOON.parse().unwrap();
    assert_eq!(
        report(&announced(Outcome::Held {
            by: "github/1/1".to_owned(),
            until
        })),
        "r: run github/1/1 holds it until 2026-10-31T12:00:00Z; nothing sent by this run"
    );
    assert_eq!(
        report(&announced(Outcome::Stopped {
            landed: vec![Step::Post, Step::Chunk(1)],
            remaining: Vec::new(),
            reason: Stop::RetryAfter(jiff::SignedDuration::from_secs(90)),
        })),
        "r: stopped: the destination asked to wait 1m 30s. Landed: post, chunk 1. The next run sends what remains: nothing."
    );
    for reason in [
        Stop::Unreachable("u".to_owned()),
        Stop::Store("s".to_owned()),
        Stop::Template("t".to_owned()),
        Stop::Marker("m".to_owned()),
    ] {
        let line = report(&announced(Outcome::Stopped {
            landed: Vec::new(),
            remaining: vec![Step::Post],
            reason,
        }));
        assert!(line.starts_with("r: stopped: "), "{line}");
    }
    let all = [
        Step::Post,
        Step::Crosspost,
        Step::OpenThread,
        Step::Chunk(2),
        Step::Pin,
        Step::Unpin,
        Step::Deliver("https://a.example/inbox".to_owned()),
    ];
    assert_eq!(
        steps(&all),
        "post, crosspost, thread, chunk 2, pin, unpin, delivery to https://a.example/inbox"
    );
}

#[test]
fn a_warning_on_actions_stays_one_workflow_command() {
    assert_eq!(
        escaped("100% done\r\n::error::forged"),
        "100%25 done%0D%0A::error::forged"
    );
    let mut setup = setup(&CONFIG.replace("kind = \"git-branch\"", MARKERS).replace(
        "template = \"release\"\n",
        "template = \"release\"\npin = \"store\"\n",
    ));
    setup
        .vars
        .insert("GITHUB_ACTIONS".to_owned(), "true".to_owned());
    setup.vars.insert(
        "GITHUB_EVENT_NAME".to_owned(),
        "push\n::error::forged".to_owned(),
    );
    let warned = herald(&setup, &World::default(), &ANNOUNCE);
    assert_eq!(
        warned
            .out
            .lines()
            .filter(|line| line.starts_with("::") && !line.starts_with("::add-mask::"))
            .count(),
        1,
        "{}",
        warned.out
    );
    assert!(
        warned
            .out
            .contains("The run's event is push ::error::forged, not a release"),
        "{}",
        warned.out
    );
}

#[test]
fn a_release_named_by_its_tag_is_announced_in_place_of_the_run_s_event() {
    let mut evented = setup(CONFIG);
    evented
        .vars
        .insert("GITHUB_EVENT_NAME".to_owned(), "push".to_owned());
    let world = World::default();
    with_release(&world, 200, false);
    discord(&world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
    let released = ["announce", "--from", "github", "--release", "v1"];
    let announced = herald(&evented, &world, &released);
    assert_eq!(
        (
            announced.code,
            announced.out.as_str(),
            announced.err.as_str()
        ),
        (DONE, "releases: announced (post)\n", "")
    );
    let drafted = World::default();
    with_release(&drafted, 200, true);
    assert_eq!(
        herald(&evented, &drafted, &released).out,
        "The release v1 is a draft; nothing to announce.\n"
    );
    let missing = World::default();
    with_release(&missing, 404, false);
    assert_eq!(
        herald(&evented, &missing, &released).err,
        "GitHub's release was answered with 404\n"
    );
    let tokened = setup(&CONFIG.replace(
        "repository = \"cli/cli\"",
        "repository = \"cli/cli\"\ntoken = { secret = \"CLI_TOKEN\" }",
    ));
    assert_eq!(
        herald(&tokened, &World::default(), &released).err,
        "CLI_TOKEN is not set; the source names it\n"
    );
}

#[test]
fn on_actions_every_secret_is_masked_before_anything_is_printed() {
    let mut setup = setup(CONFIG);
    setup
        .vars
        .insert("GITHUB_ACTIONS".to_owned(), "true".to_owned());
    setup.vars.insert(
        "HERALD_DISCORD_TOKEN".to_owned(),
        "first line\nsecond 100%".to_owned(),
    );
    setup
        .vars
        .insert("GITHUB_EVENT_NAME".to_owned(), "push".to_owned());
    let masked = herald(&setup, &World::default(), &ANNOUNCE);
    assert_eq!(
        masked.out,
        [
            "::add-mask::first line%0Asecond 100%25",
            "::add-mask::github-token",
            "::add-mask::second 100%25",
            "::add-mask::first line",
            "The run's event is push, not a release; nothing to announce.",
            "",
        ]
        .join("\n")
    );
    setup.vars.remove("GITHUB_ACTIONS");
    let local = herald(&setup, &World::default(), &ANNOUNCE);
    assert!(!local.out.contains("add-mask"), "{}", local.out);
}
