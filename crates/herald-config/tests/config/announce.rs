//! `[announce]`, `[run]`, `[cli]` and `[log]`: how a run holds and keeps an
//! announcement, how many routes and sources it goes through at once, and
//! what herald's commands print, read with their defaults, and every value
//! they cannot keep named by key.

use std::num::NonZeroUsize;

use herald_config::{Announce, Cli, Level, Log, LogFormat, Run, load};
use jiff::SignedDuration;

const EXAMPLE: &str = r#"version = 1

[store]
kind = "markers"

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "release"

[templates.release]
title = "{{ event.project }} {{ event.attributes.version }}"
body = "{{ event.body }}"
"#;

fn nothing(name: &str) -> Result<String, String> {
    Err(format!("{name}: no such file"))
}

#[test]
fn a_run_holds_a_claim_twenty_minutes_and_writes_the_latest_three_times_by_default() {
    let config = load("herald.toml", EXAMPLE, &nothing, &crate::common::kinds()).unwrap();
    assert_eq!(
        config.announce,
        Announce {
            lease: SignedDuration::from_mins(20),
            latest_attempts: 3,
        }
    );
}

#[test]
fn every_announce_setting_is_read_and_a_mistake_named_by_its_key() {
    let text = format!("{EXAMPLE}\n[announce]\nlease = \"45m\"\nlatest_attempts = 5\n");
    let config = load("herald.toml", &text, &nothing, &crate::common::kinds()).unwrap();
    assert_eq!(
        config.announce,
        Announce {
            lease: SignedDuration::from_mins(45),
            latest_attempts: 5,
        }
    );
    for (written, key) in [
        ("latest_attempts = 0", "announce.latest_attempts"),
        ("lease = \"soon\"", "announce.lease"),
        ("retries = 2", "retries"),
    ] {
        let error = load(
            "herald.toml",
            &format!("{EXAMPLE}\n[announce]\n{written}\n"),
            &nothing,
            &crate::common::kinds(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn a_run_goes_through_four_routes_and_four_sources_at_once_by_default() {
    let config = load("herald.toml", EXAMPLE, &nothing, &crate::common::kinds()).unwrap();
    assert_eq!(
        config.run,
        Run {
            routes: NonZeroUsize::new(4).unwrap(),
            sources: NonZeroUsize::new(4).unwrap(),
        }
    );
    assert_eq!(config.run, Run::default());
}

#[test]
fn every_run_setting_is_read_and_a_mistake_named_by_its_key() {
    let text = format!("{EXAMPLE}\n[run]\nroutes = 1\nsources = 9\n");
    let config = load("herald.toml", &text, &nothing, &crate::common::kinds()).unwrap();
    assert_eq!(
        config.run,
        Run {
            routes: NonZeroUsize::MIN,
            sources: NonZeroUsize::new(9).unwrap(),
        }
    );
    for (written, key) in [
        ("routes = 0", "run.routes"),
        ("sources = 0", "run.sources"),
        ("jobs = 2", "jobs"),
    ] {
        let error = load(
            "herald.toml",
            &format!("{EXAMPLE}\n[run]\n{written}\n"),
            &nothing,
            &crate::common::kinds(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn the_commands_withhold_private_rows_unless_the_cli_setting_says_their_output_is_private() {
    let config = load("herald.toml", EXAMPLE, &nothing, &crate::common::kinds()).unwrap();
    assert_eq!(config.cli, Cli { private: false });
    let text = format!("{EXAMPLE}\n[cli]\nprivate = true\n");
    let config = load("herald.toml", &text, &nothing, &crate::common::kinds()).unwrap();
    assert_eq!(config.cli, Cli { private: true });
    for (written, key) in [
        ("private = \"yes\"", "cli.private"),
        ("advisories = true", "advisories"),
    ] {
        let error = load(
            "herald.toml",
            &format!("{EXAMPLE}\n[cli]\n{written}\n"),
            &nothing,
            &crate::common::kinds(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn the_log_s_level_format_and_least_scrubbed_length_are_read_and_a_mistake_named_by_its_key() {
    let config = load("herald.toml", EXAMPLE, &nothing, &crate::common::kinds()).unwrap();
    assert_eq!(config.log, Log::default());
    assert_eq!(
        Log::default(),
        Log {
            level: Level::Info,
            format: LogFormat::Text,
            scrub_from: NonZeroUsize::new(8).unwrap(),
        }
    );
    let text = format!("{EXAMPLE}\n[log]\nlevel = \"warn\"\nformat = \"json\"\nscrub_from = 4\n");
    let config = load("herald.toml", &text, &nothing, &crate::common::kinds()).unwrap();
    assert_eq!(
        config.log,
        Log {
            level: Level::Warn,
            format: LogFormat::Json,
            scrub_from: NonZeroUsize::new(4).unwrap(),
        }
    );
    let text = format!("{EXAMPLE}\n[log]\nlevel = \"error\"\n");
    let config = load("herald.toml", &text, &nothing, &crate::common::kinds()).unwrap();
    assert_eq!(config.log.level, Level::Error);
    for (written, key) in [
        ("level = \"debug\"", "log.level"),
        ("format = \"yaml\"", "log.format"),
        ("scrub_from = 0", "log.scrub_from"),
        ("colour = true", "colour"),
    ] {
        let error = load(
            "herald.toml",
            &format!("{EXAMPLE}\n[log]\n{written}\n"),
            &nothing,
            &crate::common::kinds(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn a_level_and_a_format_are_named_as_the_config_writes_them() {
    assert!(Level::Info < Level::Warn && Level::Warn < Level::Error);
    for level in Level::ALL {
        assert_eq!(Level::named(level.name()), Some(level));
    }
    assert_eq!(Level::ALL.map(Level::name), ["info", "warn", "error"]);
    assert_eq!(Level::named("debug"), None);
    for format in LogFormat::ALL {
        assert_eq!(LogFormat::named(format.name()), Some(format));
    }
    assert_eq!(LogFormat::ALL.map(LogFormat::name), ["text", "json"]);
    assert_eq!(LogFormat::named("yaml"), None);
}
