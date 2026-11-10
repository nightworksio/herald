//! `[store]`'s `keep_*` keys: how long the store keeps what herald records,
//! read with their defaults, and every value it cannot keep named by key.

mod common;

#[cfg(test)]
mod retention {
    use herald_config::load;
    use herald_core::Retention;
    use jiff::SignedDuration;

    const REST: &str = r#"
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

    fn config(store: &str) -> Result<herald_config::Config, String> {
        load(
            "herald.toml",
            &format!("version = 1\n\n[store]\n{store}\n{REST}"),
            &|name| Err(format!("{name}: no such file")),
            &crate::common::kinds(),
        )
        .map_err(|errors| errors.to_string())
    }

    fn days(count: i64) -> SignedDuration {
        SignedDuration::from_hours(24 * count)
    }

    #[test]
    fn a_store_keeps_deliveries_a_week_entries_90_days_and_notes_for_ever_by_default() {
        let config = config("kind = \"sqlite\"").unwrap();
        assert_eq!(
            config.retention,
            Retention {
                deliveries: days(7),
                ledger: Some(days(90)),
                notes: None,
            }
        );
        assert_eq!(config.retention, Retention::default());
    }

    #[test]
    fn every_keep_setting_is_read_beside_the_store_s_own_options() {
        let sqlite = config(
            "kind = \"sqlite\"\npath = \"state.sqlite\"\nkeep_deliveries = \"3d\"\nkeep_ledger = \"forever\"\nkeep_notes = \"2w\"",
        )
        .unwrap();
        assert_eq!(
            sqlite.retention,
            Retention {
                deliveries: days(3),
                ledger: None,
                notes: Some(days(14)),
            }
        );
        let Some(options) = sqlite.store.of::<herald_store_sqlite::StoreOptions>() else {
            panic!("not a sqlite store: {:?}", sqlite.store);
        };
        assert_eq!(options.path.as_deref(), Some("state.sqlite"));
        let branch = config(
            "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }\nkeep_ledger = \"30d\"\nkeep_notes = \"forever\"",
        )
        .unwrap();
        assert_eq!(branch.retention.ledger, Some(days(30)));
        assert_eq!(branch.retention.notes, None);
    }

    #[test]
    fn a_keep_value_herald_cannot_keep_is_named_by_its_key() {
        let short = config("kind = \"sqlite\"\nkeep_deliveries = \"2d\"").unwrap_err();
        assert!(short.contains("store.keep_deliveries"), "{short}");
        assert!(short.contains("at least \"3d\""), "{short}");
        let never = config("kind = \"sqlite\"\nkeep_deliveries = \"forever\"").unwrap_err();
        assert!(never.contains("keep_deliveries"), "{never}");
        let soon = config("kind = \"sqlite\"\nkeep_ledger = \"soon\"").unwrap_err();
        assert!(soon.contains("keep_ledger"), "{soon}");
        assert!(soon.contains("\"90d\", or \"forever\""), "{soon}");
        let markers = config("kind = \"markers\"\nkeep_notes = \"7d\"").unwrap_err();
        assert!(markers.contains("keep_notes"), "{markers}");
    }

    #[test]
    fn every_store_but_markers_keeps_records_as_long_as_its_table_and_its_schema_say() {
        let variables = config(
            "kind = \"repo-variables\"\ntoken = { secret = \"HERALD_STATE_TOKEN\" }\nkey = { secret = \"HERALD_STORE_KEY\" }\nkeep_ledger = \"30d\"",
        )
        .unwrap();
        assert_eq!(variables.retention.ledger, Some(days(30)));
        let schema: serde_json::Value =
            serde_json::from_str(&herald_config::schema(&crate::common::kinds())).unwrap();
        let stores = schema["$defs"]["Store"]["oneOf"].as_array().unwrap();
        assert_eq!(stores.len(), crate::common::kinds().store_kinds().len());
        for store in stores {
            let kind = store["properties"]["kind"]["const"].as_str().unwrap();
            for key in ["keep_deliveries", "keep_ledger", "keep_notes"] {
                assert_eq!(
                    store["properties"].get(key).is_some(),
                    kind != "markers",
                    "{kind} {key}"
                );
            }
        }
    }
}
