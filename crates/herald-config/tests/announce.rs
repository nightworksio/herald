//! `[announce]`: how a run holds and keeps an announcement, read with its
//! defaults, and every value it cannot keep named by key.

mod common;

#[cfg(test)]
mod announce {
    use herald_config::{Announce, load};
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
}
