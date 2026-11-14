//! An owner source's keys, `[doctor] private_to_public` and a route's
//! `private`, read with their defaults, and every value they cannot keep
//! named by key.

mod common;

#[cfg(test)]
mod owner {
    use herald_config::{Doctor, Guard, load};
    use herald_core::Visibility;
    use herald_sources_forge::ForgeSource;

    const EXAMPLE: &str = r#"version = 1

[store]
kind = "sqlite"

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.org]
kind = "gitlab"
owner = "lemonfiber/plugins"
include = ["plugin-*"]
visibility = ["public", "internal"]
properties = { team = "media", tier = ["core", "edge"] }
subgroups = false
keep_members = "7d"

[[routes]]
id = "releases"
sources = ["org"]
destination = "discord"
channel = "1566200000000000100"
template = "release"
"#;

    const TEMPLATES: &str = r#"
[templates.release]
title = "{{ event.project }} {{ event.attributes.version }}"
body = "{{ event.body }}"
"#;

    fn nothing(name: &str) -> Result<String, String> {
        Err(format!("{name}: no such file"))
    }

    fn loaded(route: &str, rest: &str) -> Result<herald_config::Config, String> {
        load(
            "herald.toml",
            &format!("{EXAMPLE}{route}\n{TEMPLATES}{rest}"),
            &nothing,
            &crate::common::kinds(),
        )
        .map_err(|error| error.to_string())
    }

    #[test]
    fn an_owner_source_reads_its_selection_and_a_route_is_public_where_it_says_nothing() {
        let config = loaded("", "").unwrap();
        let source = config.sources.values().next().unwrap();
        let forged = source.of::<ForgeSource>().unwrap();
        assert_eq!(forged.options.owner.as_deref(), Some("lemonfiber/plugins"));
        assert_eq!(
            forged.options.visibility,
            Some(vec![Visibility::Public, Visibility::Internal])
        );
        assert!(source.unlisted());
        assert!(!config.routes[0].private);
        assert_eq!(config.doctor, Doctor::default());
        assert_eq!(config.doctor.private_to_public, Guard::Refuse);
    }

    #[test]
    fn a_route_declares_its_place_private_and_doctor_takes_how_it_judges_one_that_is_not() {
        let config = loaded(
            "private = true",
            "\n[doctor]\nprivate_to_public = \"warn\"\n",
        )
        .unwrap();
        assert!(config.routes[0].private);
        assert_eq!(config.doctor.private_to_public, Guard::Warn);
        let allowed = loaded("", "\n[doctor]\nprivate_to_public = \"allow\"\n").unwrap();
        assert_eq!(allowed.doctor.private_to_public, Guard::Allow);
        for (rest, key) in [
            ("\n[doctor]\nprivate_to_public = \"never\"\n", "doctor"),
            ("\n[doctor]\nstrict = true\n", "doctor"),
        ] {
            let error = loaded("", rest).unwrap_err();
            assert!(error.contains(key), "{error}");
        }
    }

    #[test]
    fn an_owner_source_s_mistakes_are_named() {
        let error = load(
            "herald.toml",
            &format!(
                "{}{TEMPLATES}",
                EXAMPLE.replace(
                    "subgroups = false\n",
                    "subgroups = false\nrepository = \"a/b\"\n"
                )
            ),
            &nothing,
            &crate::common::kinds(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("a source names either a repository or an owner; remove one"),
            "{error}"
        );
    }
}
