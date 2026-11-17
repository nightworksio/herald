//! A source whose cursor remembers every identity it lists is refused where
//! the store keeps no cursor, or keeps records smaller than it may grow
//! (ADR-0042).

mod common;

#[cfg(test)]
mod remembering {
    use herald_config::load;

    fn text(store: &str, source: &str) -> String {
        format!(
            r#"version = 1

[store]
{store}

[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}

[sources.app]
kind = "oci"
registry = "https://registry.example"
{source}

[[routes]]
id = "releases"
sources = ["app"]
destination = "discord"
channel = "1566200000000000100"
template = "release"

[templates.release]
title = "{{{{ event.project }}}} {{{{ event.attributes.version }}}}"
body = "{{{{ event.body }}}}"
"#
        )
    }

    fn config(store: &str, source: &str) -> Result<(), String> {
        let text = text(store, source);
        load(
            "herald.toml",
            &text,
            &|_| Err(String::new()),
            &crate::common::kinds(),
        )
        .map(|_| ())
        .map_err(|errors| errors.to_string())
    }

    const APP: &str = "repository = \"acme/app\"";

    #[test]
    fn a_store_keeping_records_of_any_size_keeps_an_oci_cursor() {
        assert_eq!(config("kind = \"sqlite\"", APP), Ok(()));
        assert_eq!(
            config(
                "kind = \"sqlite\"",
                "catalog = true\nmost_tags = 100000\nremembered = 100000"
            ),
            Ok(())
        );
    }

    #[test]
    fn a_store_keeping_no_cursor_refuses_an_oci_source() {
        let refused = config("kind = \"markers\"", APP).unwrap_err();
        assert!(
            refused.contains("sources.app.most_tags: the markers store keeps no cursor, and this source remembers every identity it lists; give a store that keeps records, such as \"git-branch\""),
            "{refused}"
        );
    }

    #[test]
    fn a_store_keeping_small_records_refuses_an_oci_source_whose_named_bound_outgrows_them() {
        let each = "registry.example/acme/app/".len() + 128 + 5;
        let fitting = (65_536 - 4_096) / each;
        let refused =
            config("kind = \"deno-kv\"", &format!("{APP}\nmost_tags = 10000")).unwrap_err();
        assert!(
            refused.contains(&format!(
                "sources.app.most_tags: the deno-kv store keeps at most 65536 bytes in a record, and this source's cursor may take {} remembering 10000 identities; make most_tags at most {fitting}, or give a store that keeps more",
                10_000 * each + 4_096
            )),
            "{refused}"
        );
        assert!(
            config(
                "kind = \"deno-kv\"",
                &format!("{APP}\nmost_tags = {fitting}")
            )
            .is_ok()
        );
        let remembered = config(
            "kind = \"deno-kv\"",
            &format!("{APP}\nmost_tags = 10\nremembered = {}", fitting + 1),
        )
        .unwrap_err();
        assert!(
            remembered
                .contains("sources.app.remembered: the deno-kv store keeps at most 65536 bytes"),
            "{remembered}"
        );
        let paged = config(
            "kind = \"deno-kv\"",
            &format!("{APP}\npage = {}", fitting + 1),
        )
        .unwrap_err();
        assert!(
            paged.contains(&format!("sources.app.most_tags: the deno-kv store keeps at most 65536 bytes in a record, and this source's cursor may take {} remembering 10000 identities", 10_000 * each + 4_096)),
            "{paged}"
        );
    }

    #[test]
    fn an_oci_source_naming_no_bound_remembers_the_most_tags_its_store_s_records_hold() {
        let each = "registry.example/acme/app/".len() + 128 + 5;
        let fitted = |store: &str| {
            let text = text(store, APP);
            let config = load(
                "herald.toml",
                &text,
                &|_| Err(String::new()),
                &crate::common::kinds(),
            )
            .unwrap();
            let source = config.sources.values().next().unwrap();
            let Some(oci) = source.of::<herald_sources_registry::OciSource>() else {
                panic!("an oci source");
            };
            (
                oci.options.most_tags.map(std::num::NonZeroUsize::get),
                oci.fitted,
            )
        };
        assert_eq!(
            fitted("kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }"),
            (Some((1_000_000 - 4_096) / each), true)
        );
        assert_eq!(
            fitted("kind = \"deno-kv\""),
            (Some((65_536 - 4_096) / each), true)
        );
        assert_eq!(fitted("kind = \"sqlite\""), (None, false));
        assert_eq!(fitted("kind = \"d1\"\nbinding = \"DB\""), (None, false));
        let explicit = config(
            "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }",
            &format!("{APP}\nmost_tags = 10000"),
        )
        .unwrap_err();
        assert!(
            explicit.contains("sources.app.most_tags: the git-branch store keeps at most 1000000 bytes in a record"),
            "{explicit}"
        );
    }

    #[test]
    fn a_source_remembering_the_newest_few_is_kept_by_any_store() {
        let text = r#"version = 1

[store]
kind = "deno-kv"

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
title = "{{ event.project }}"
body = "{{ event.body }}"
"#;
        assert!(
            load(
                "herald.toml",
                text,
                &|_| Err(String::new()),
                &crate::common::kinds()
            )
            .is_ok()
        );
    }
}
