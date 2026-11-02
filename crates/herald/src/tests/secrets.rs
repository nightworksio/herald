use std::io::Write;

use herald_ports::Secret;

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
            "/channels/100/messages",
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
fn a_line_left_unfinished_is_scrubbed_when_the_writer_is_flushed() {
    let hidden = vec![
        Secret::new("LONG", "secret-value-long"),
        Secret::new("SHORT", "secret-value"),
        Secret::new("ODD", "ééééééééé"),
        Secret::new("TINY", "one"),
    ];
    let mut written = Vec::new();
    let mut scrubbing = Scrubbing::new(&mut written, &hidden);
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
            ("HERALD_DISCORD_TOKEN", "second-line"),
            ("HERALD_DISCORD_TOKEN", "first"),
            ("CI_JOB_TOKEN", "job"),
        ]
    );
}
