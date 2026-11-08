//! `[server]`: how `herald server` listens and what it takes on at once,
//! read with its defaults, and every value it cannot serve named by key.

#[cfg(test)]
mod server {
    use herald_config::{KeyCache, Server, Source, load};

    const EXAMPLE: &str = r#"version = 1

[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }

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
title = "{{ event.project }} {{ event.version }}"
body = "{{ event.body }}"
"#;

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn errors(text: &str) -> String {
        load("herald.toml", text, &nothing).unwrap_err().to_string()
    }

    #[test]
    fn a_config_without_a_server_table_serves_with_the_defaults() {
        let config = load("herald.toml", EXAMPLE, &nothing).unwrap();
        assert_eq!(config.server, Server::default());
        assert_eq!(config.server.listen, "127.0.0.1:8080");
        assert_eq!(
            (
                config.server.jobs,
                config.server.connections,
                config.server.body,
                config.server.fetches,
                config.server.completions,
                config.server.deliveries,
                config.server.keys_most,
            ),
            (8, 256, 1_048_576, 4, 4, 8, 1024)
        );
        assert_eq!(config.server.keys, KeyCache::Memory);
        assert!(!config.server.keys_in_store());
        assert_eq!(config.server.keys_refreshed.as_secs(), 3_600);
        assert_eq!(config.server.keys_reread.as_secs(), 60);
        assert_eq!(config.server.silence.as_secs(), 30);
    }

    #[test]
    fn the_server_table_sets_every_limit() {
        let text = format!(
            "{EXAMPLE}\n[server]\nlisten = \"0.0.0.0:9000\"\nbase = \"/tools/herald\"\njobs = 2\nconnections = 3\nbody = \"64KiB\"\nfetches = 5\ncompletions = 9\ndeliveries = 6\nsilence = \"45s\"\n\n[server.keys]\ncache = \"store\"\nrefresh_after = \"30m\"\nreread_after = \"5m\"\nmost = 7\ndocument = \"1MiB\"\n\n[server.inbound]\nreplay_window = \"20m\"\nsignatures = 2\ncovered = 4\n"
        );
        let config = load("herald.toml", &text, &nothing).unwrap();
        assert_eq!(
            config.server,
            Server {
                listen: "0.0.0.0:9000".to_owned(),
                base: "/tools/herald".to_owned(),
                jobs: 2,
                connections: 3,
                body: 65_536,
                fetches: 5,
                completions: 9,
                deliveries: 6,
                silence: jiff::SignedDuration::from_secs(45),
                keys: KeyCache::Store,
                keys_refreshed: jiff::SignedDuration::from_mins(30),
                keys_reread: jiff::SignedDuration::from_mins(5),
                keys_most: 7,
                inbound: herald_core::Inbound {
                    replay_window: jiff::SignedDuration::from_mins(20),
                    signatures: 2,
                    covered: 4,
                    document: 1_048_576,
                },
            }
        );
        assert!(config.server.keys_in_store());
        let memory = format!("{EXAMPLE}\n[server.keys]\ncache = \"memory\"\n");
        let config = load("herald.toml", &memory, &nothing).unwrap();
        assert_eq!(config.server.keys, KeyCache::Memory);
    }

    #[test]
    fn a_server_table_that_cannot_be_served_is_refused_by_key() {
        let unbound = errors(&format!("{EXAMPLE}\n[server]\nlisten = \"localhost\"\n"));
        assert!(
            unbound.contains(
                "server.listen: \"localhost\" is not an address and a port; write one such as \"127.0.0.1:8080\""
            ),
            "{unbound}"
        );
        let idle = errors(&format!("{EXAMPLE}\n[server]\njobs = 0\n"));
        assert!(idle.contains("server.jobs"), "{idle}");
        let huge = errors(&format!("{EXAMPLE}\n[server]\nbody = \"1GB\"\n"));
        assert!(huge.contains("server.body"), "{huge}");
        assert!(huge.contains("\"512KiB\" or \"1MiB\""), "{huge}");
        let wide = errors(&format!("{EXAMPLE}\n[server]\nbody = \"5MiB\"\n"));
        assert!(
            wide.contains("server.body: make it at most \"4MiB\", four times its default"),
            "{wide}"
        );
        for (key, value, most) in [
            ("replay_window", "\"21m\"", "\"20m\""),
            ("signatures", "17", "16"),
            ("covered", "65", "64"),
        ] {
            let past = errors(&format!("{EXAMPLE}\n[server.inbound]\n{key} = {value}\n"));
            assert!(
                past.contains(&format!("server.inbound.{key}: make it at most {most}")),
                "{past}"
            );
        }
        let heavy = errors(&format!("{EXAMPLE}\n[server.keys]\ndocument = \"2MiB\"\n"));
        assert!(
            heavy.contains("server.keys.document: make it at most \"1MiB\""),
            "{heavy}"
        );
        for base in ["herald", "/herald/", "/", "/a//b", "/../x", "/he rald"] {
            let wrong = errors(&format!("{EXAMPLE}\n[server]\nbase = \"{base}\"\n"));
            assert!(
                wrong.contains(&format!(
                    "server.base: \"{base}\" is not a path to serve under"
                )),
                "{wrong}"
            );
        }
        let bare = errors(&format!("{EXAMPLE}\n[server.inbound]\ncovered = 3\n"));
        assert!(
            bare.contains(
                "server.inbound.covered: an ActivityPub signature covers at least 4 headers"
            ),
            "{bare}"
        );
        let unkept = errors(&format!(
            "{}\n[server.keys]\ncache = \"store\"\n",
            EXAMPLE.replace(
                "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }",
                "kind = \"markers\""
            )
        ));
        assert!(
            unkept.contains(
                "server.keys.cache: the markers store keeps no keys; keep them in \"memory\", or give a store that keeps records"
            ),
            "{unkept}"
        );
        let cached = |keeps: &str| {
            format!(
                "{}\n[server.keys]\ncache = \"store\"\n",
                EXAMPLE.replace(
                    "kind = \"git-branch\"",
                    &format!("kind = \"actions-cache\"\n{keeps}")
                )
            )
        };
        let uncached = errors(&cached(""));
        assert!(
            uncached.contains(
                "server.keys.cache: the actions-cache store keeps no keys; keep them in \"memory\", or add \"keys\" to its keeps"
            ),
            "{uncached}"
        );
        assert!(load("herald.toml", &cached("keeps = [\"keys\"]"), &nothing).is_ok());
        let stray = errors(&format!("{EXAMPLE}\n[server]\nport = 80\n"));
        assert!(stray.contains("server.port"), "{stray}");
    }

    #[test]
    fn a_forge_source_names_the_secret_its_webhooks_are_signed_with() {
        let hooked = EXAMPLE.replace(
            "repository = \"nightworksio/php-mutation-gate\"",
            "repository = \"nightworksio/php-mutation-gate\"\nwebhook = { secret = \"HERALD_WEBHOOK\" }",
        );
        let config = load("herald.toml", &hooked, &nothing).unwrap();
        assert!(config.secrets().contains("HERALD_WEBHOOK"));
        assert!(matches!(
            config.sources.values().next(),
            Some(Source::Forge(_, options))
                if options.webhook.as_ref().map(|named| named.secret.as_str()) == Some("HERALD_WEBHOOK")
        ));
    }
}
