//! Each source's `page`, `remembered` and `parse`: read with their
//! defaults, and a reading that cannot be kept named by its source.

mod common;

#[cfg(test)]
mod reading {
    use herald_config::load;
    use herald_core::SourceName;

    fn config(source: &str) -> Result<herald_config::Config, String> {
        let text = format!(
            r#"version = 1

[store]
kind = "markers"

[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}

[sources.gate]
{source}

[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "release"

[templates.release]
title = "{{{{ event.project }}}} {{{{ event.attributes.version }}}}"
body = "{{{{ event.body }}}}"
"#
        );
        load(
            "herald.toml",
            &text,
            &|name| Err(format!("{name}: no such file")),
            &crate::common::kinds(),
        )
        .map_err(|errors| errors.to_string())
    }

    #[test]
    fn a_source_reads_its_page_remembers_its_count_and_parses_its_most() {
        let read = config(
            "kind = \"github\"\nrepository = \"nightworksio/php-mutation-gate\"\npage = 50\nremembered = 1000\nparse = \"8MiB\"",
        )
        .unwrap();
        let Some(forged) = read
            .sources
            .get(&SourceName::new("gate"))
            .and_then(|source| source.of::<herald_sources_forge::ForgeSource>())
        else {
            panic!("no forge source");
        };
        let reading = forged.options.reading();
        assert_eq!(
            (reading.page, reading.remembered, reading.parse),
            (50, 1000, 8 << 20)
        );
        let npm = config("kind = \"npm\"\npackage = \"left-pad\"\nparse = \"128MiB\"").unwrap();
        let Some(listed) = npm
            .sources
            .get(&SourceName::new("gate"))
            .and_then(|source| source.of::<herald_sources_registry::RegistrySource>())
        else {
            panic!("no registry source");
        };
        assert_eq!(listed.options.reading(listed.registry).parse, 128 << 20);
    }

    #[test]
    fn a_reading_that_cannot_be_kept_is_named_by_its_source() {
        let forgetful = config(
            "kind = \"github\"\nrepository = \"nightworksio/php-mutation-gate\"\npage = 50\nremembered = 10",
        )
        .unwrap_err();
        assert!(
            forgetful.contains(
                "sources.gate: the cursor remembers 10 identities, fewer than the 50 one poll reads"
            ),
            "{forgetful}"
        );
        let greedy =
            config("kind = \"feed\"\nurl = \"https://blog.example/feed.xml\"\nparse = \"17MiB\"")
                .unwrap_err();
        assert!(
            greedy.contains("sources.gate: make parse at most \"16MiB\", four times its default"),
            "{greedy}"
        );
        let packagist =
            config("kind = \"packagist\"\npackage = \"vendor/name\"\nparse = \"65MiB\"")
                .unwrap_err();
        assert!(
            packagist.contains("make parse at most \"64MiB\""),
            "{packagist}"
        );
    }
}
