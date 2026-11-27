use std::io::Write;

use herald_ports::{Scrubber, Secret};

use super::world::*;
use crate::*;

const TOKENS: [&str; 2] = ["discord-token", "github-token"];

fn clean(run: &Run) {
    for token in TOKENS {
        assert!(!run.out.contains(token), "{token} in {}", run.out);
        assert!(!run.err.contains(token), "{token} in {}", run.err);
    }
}

#[test]
fn a_failing_run_never_prints_a_token_even_where_a_server_echoes_it() {
    let setup = setup(CONFIG);
    let world = World::default();
    discord(
        &world,
        &[(
            "/channels/1566200000000000100/messages",
            401,
            r#"{"code":0,"message":"401: Unauthorized for Bot discord-token"}"#,
        )],
    );
    let refused = herald(&setup, &world, &ANNOUNCE);
    assert_eq!(refused.code, STOPPED);
    assert!(
        refused.out.contains("Bot «secret:HERALD_DISCORD_TOKEN»"),
        "{}",
        refused.out
    );
    clean(&refused);
    let unreachable = super::world::setup(&CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"git-branch\"\napi = \"https://unreachable.example\"",
    ));
    let stopped = herald(&unreachable, &world, &ANNOUNCE);
    assert_eq!(stopped.code, STOPPED);
    clean(&stopped);
    let dry = herald(
        &setup,
        &world,
        &["announce", "--from", "github", "--dry-run"],
    );
    clean(&dry);
}

#[test]
fn a_command_that_reads_the_config_writes_through_the_scrubber() {
    let mut setup = setup(CONFIG);
    setup
        .vars
        .insert("HERALD_ROUND_SECRET".to_owned(), "git-branch".to_owned());
    let stored = herald(&setup, &World::default(), &["store"]);
    assert_eq!(stored.out, "«secret:HERALD_ROUND_SECRET»\n");
}

#[test]
fn a_line_left_unfinished_is_scrubbed_when_the_writer_is_flushed() {
    let hidden = vec![
        Secret::new("LONG", "secret-value-long"),
        Secret::new("SHORT", "secret-value"),
        Secret::new("ODD", "ééééééééé"),
        Secret::new("TINY", "one"),
    ];
    let scrubber = Scrubber::new(herald_config::Log::SCRUB_FROM);
    for secret in &hidden {
        scrubber.keep(secret);
    }
    let mut written = Vec::new();
    let mut scrubbing = Scrubbing::new(&mut written, scrubber);
    write!(scrubbing, "one secret-value-long\ntwo secret-").unwrap();
    write!(scrubbing, "valueéééééééééé ").unwrap();
    write!(scrubbing, "end").unwrap();
    scrubbing.flush().unwrap();
    scrubbing.flush().unwrap();
    assert_eq!(
        String::from_utf8(written).unwrap(),
        "one «secret:LONG»\ntwo «secret:SHORT»«secret:ODD»é end"
    );
}

#[test]
fn every_secret_the_run_may_resolve_is_hidden_the_longest_first_line_by_line() {
    let config = read(
        &setup(CONFIG)
            .directory
            .path()
            .join("herald.toml")
            .display()
            .to_string(),
    )
    .unwrap();
    let vars: BTreeMap<String, String> = [
        ("HERALD_DISCORD_TOKEN", "first\nsecond-line"),
        ("GITHUB_TOKEN", "  "),
        ("CI_JOB_TOKEN", "job"),
        ("HERALD_ROUND_SECRET", "round-secret"),
        ("AWS_SECRET_ACCESS_KEY", "aws-secret-key"),
        ("UNRELATED", "kept"),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value.to_owned()))
    .collect();
    let found = hidden(&config, &vars);
    let named: Vec<(&str, &str)> = found
        .iter()
        .map(|secret| (secret.name(), secret.expose()))
        .collect();
    assert_eq!(
        named,
        [
            ("HERALD_DISCORD_TOKEN", "first\nsecond-line"),
            ("AWS_SECRET_ACCESS_KEY", "aws-secret-key"),
            ("HERALD_ROUND_SECRET", "round-secret"),
            ("HERALD_DISCORD_TOKEN", "second-line"),
            ("HERALD_DISCORD_TOKEN", "first"),
            ("CI_JOB_TOKEN", "job"),
        ]
    );
}

/// The config with its source reading as the GitHub App `herald`.
fn as_an_app() -> Setup {
    let config = CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"cli/cli\"\napp = \"herald\"\n",
    ) + "[github_apps.herald]\nclient_id = \"Iv23liABCDEF\"\nprivate_key = { secret = \"HERALD_APP_KEY\" }\n";
    let mut setup = setup(&config);
    setup
        .vars
        .insert("HERALD_APP_KEY".to_owned(), super::activitypub::pkcs8());
    setup
}

#[test]
fn a_token_minted_as_the_run_goes_is_scrubbed_where_a_server_echoes_it() {
    let minted = format!("ghs_{}", "M1n7".repeat(9));
    let setup = as_an_app();
    let world = World::default();
    let installed = |world: &World| {
        let mut state = world.state.lock().unwrap();
        state.forge.insert(
            "/repos/cli/cli/installation".to_owned(),
            (200, r#"{"id":42}"#.to_owned()),
        );
        state.forge.insert(
            "/app/installations/42/access_tokens".to_owned(),
            (
                201,
                format!(r#"{{"token":"{minted}","expires_at":"2099-01-01T00:00:00Z"}}"#),
            ),
        );
    };
    forge(&world, (200, &["v1"]));
    installed(&world);
    let first = herald(&setup, &world, &POLL);
    assert_eq!(first.code, DONE, "{}{}", first.out, first.err);
    forge(&world, (200, &["v2", "v1"]));
    installed(&world);
    discord(
        &world,
        &[(
            "/channels/1566200000000000100/messages",
            401,
            &format!(r#"{{"code":0,"message":"401: the forge's {minted} is not ours"}}"#),
        )],
    );
    let run = herald(&setup, &world, &POLL);
    let written = format!("{}{}", run.out, run.err);
    assert!(!written.contains(&minted), "{written}");
    assert!(written.contains("«secret:github_apps.herald»"), "{written}");
}

#[test]
fn a_json_log_at_warn_writes_what_stopped_as_objects_and_leaves_the_rest() {
    let world = World::default();
    discord(
        &world,
        &[(
            "/channels/1566200000000000100/messages",
            401,
            r#"{"code":0,"message":"401: Unauthorized for Bot discord-token"}"#,
        )],
    );
    let mut setup = setup(CONFIG);
    setup
        .vars
        .insert(herald_compose::LOG_LEVEL.to_owned(), "error".to_owned());
    let run = herald(
        &setup,
        &world,
        &[
            "announce",
            "--from",
            "github",
            "--log-format",
            "json",
            "--log-level",
            "warn",
        ],
    );
    assert_eq!(run.code, STOPPED);
    let lines: Vec<serde_json::Value> = run
        .out
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(!lines.is_empty(), "{}", run.out);
    for line in &lines {
        assert!(
            line["level"] == "warn" || line["level"] == "error",
            "{line}"
        );
        assert!(
            line["message"]
                .as_str()
                .is_some_and(|message| !message.contains("discord-token")),
            "{line}"
        );
    }
    let quiet = herald(&setup, &world, &ANNOUNCE);
    assert!(
        quiet.out.lines().all(|line| !line.starts_with('{')),
        "{}",
        quiet.out
    );
}

#[test]
fn a_log_variable_naming_no_level_stops_the_run_before_it_starts() {
    let mut setup = setup(CONFIG);
    setup
        .vars
        .insert(herald_compose::LOG_LEVEL.to_owned(), "loud".to_owned());
    let run = herald(&setup, &World::default(), &ANNOUNCE);
    assert_eq!(run.code, MISTAKEN);
    assert_eq!(
        run.err,
        "HERALD_LOG_LEVEL is \"loud\"; it takes info, warn, error\n"
    );
    let flagged = herald(
        &setup,
        &World::default(),
        &["announce", "--from", "github", "--log-level", "loud"],
    );
    assert_eq!(flagged.code, MISTAKEN);
    assert!(
        flagged.err.contains("it takes info, warn, error"),
        "{}",
        flagged.err
    );
}
