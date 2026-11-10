//! `[render]`: how far a template may go as it renders, read with its
//! defaults, held to its maximums, and every template compiled into it.

mod common;

#[cfg(test)]
mod render {
    use herald_config::load;
    use herald_core::Sandbox;

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

    fn errors(text: &str) -> String {
        load("herald.toml", text, &nothing, &crate::common::kinds())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn a_config_without_a_render_table_renders_in_the_default_sandbox() {
        let config = load("herald.toml", EXAMPLE, &nothing, &crate::common::kinds()).unwrap();
        assert_eq!(config.render, Sandbox::DEFAULT);
        assert_eq!(
            config.render,
            Sandbox {
                fuel: 100_000,
                recursion: 16,
                output: 262_144,
            }
        );
    }

    #[test]
    fn every_render_setting_is_read_and_the_templates_are_held_to_it() {
        let text =
            format!("{EXAMPLE}\n[render]\nfuel = 400000\nrecursion = 2\noutput = \"1MiB\"\n");
        let config = load("herald.toml", &text, &nothing, &crate::common::kinds()).unwrap();
        assert_eq!(
            config.render,
            Sandbox {
                fuel: 400_000,
                recursion: 2,
                output: 1_048_576,
            }
        );
        let small = errors(&format!("{EXAMPLE}\n[render]\noutput = \"1KiB\"\n"));
        assert!(
            small.contains(
                "templates.release.body: it does not render a sample release event: its output passes the cap of 1024 bytes"
            ),
            "{small}"
        );
    }

    #[test]
    fn a_render_bound_past_four_times_its_default_is_named_by_its_key() {
        for (key, value, most) in [
            ("fuel", "400001", "400000"),
            ("recursion", "65", "64"),
            ("output", "\"2MiB\"", "\"1MiB\""),
        ] {
            let past = errors(&format!("{EXAMPLE}\n[render]\n{key} = {value}\n"));
            assert!(
                past.contains(&format!(
                    "render.{key}: make it at most {most}, four times its default"
                )),
                "{past}"
            );
        }
        let unknown = errors(&format!("{EXAMPLE}\n[render]\nspeed = 1\n"));
        assert!(unknown.contains("speed"), "{unknown}");
        let none = errors(&format!("{EXAMPLE}\n[render]\nfuel = 0\n"));
        assert!(none.contains("render.fuel"), "{none}");
    }
}
