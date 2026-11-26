//! `[[routes]] templates`: a route giving a dialect its own template, each
//! still Markdown converted to that dialect, checked as the config loads.

use herald_config::{Config, load};

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn loaded(routes: &str) -> Result<Config, String> {
    let text = format!("{HEAD}{routes}{TEMPLATES}");
    load("herald.toml", &text, &nothing, &crate::common::kinds()).map_err(|error| error.to_string())
}

const HEAD: &str = r#"version = 1

[store]
kind = "sqlite"

[destinations.chat]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[destinations.social]
kind = "mastodon"
server = "https://mastodon.example"
token = { secret = "HERALD_MASTODON_TOKEN" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
"#;

const TEMPLATES: &str = r#"
[templates.long]
title = "{{ event.title }}"
body = "{{ event.body }}"

[templates.short]
title = "{{ event.title }}"
body = "{{ event.title }}"
"#;

/// A route `id` to `destination`, rendering with `template` where it is
/// given, and with `templates` by dialect.
fn route(id: &str, destination: &str, template: Option<&str>, templates: &str) -> String {
    let channel = if destination == "chat" {
        "channel = \"1566184281406177531\"\n"
    } else {
        ""
    };
    let template = template.map_or_else(String::new, |name| format!("template = \"{name}\"\n"));
    format!(
        "\n[[routes]]\nid = \"{id}\"\nsources = [\"gate\"]\ndestination = \"{destination}\"\n{channel}{template}templates = {{ {templates} }}\n"
    )
}

fn templates(config: &Config) -> Vec<Option<&str>> {
    config
        .routes
        .iter()
        .map(|route| {
            route
                .route
                .template
                .as_ref()
                .map(herald_core::TemplateName::as_str)
        })
        .collect()
}

#[test]
fn a_dialect_with_its_own_template_renders_it_and_every_other_keeps_the_route_s() {
    let given = "mastodon = \"short\"";
    let routes = [
        route("tooted", "social", Some("long"), given),
        route("posted", "chat", Some("long"), given),
        route("alone", "social", None, given),
        route("plain", "chat", Some("long"), ""),
    ]
    .concat();
    let config = loaded(&routes).unwrap();
    assert_eq!(
        templates(&config),
        [Some("short"), Some("long"), Some("short"), Some("long")]
    );
}

#[test]
fn a_dialect_no_destination_kind_writes_is_refused_at_load() {
    let dialects =
        "the dialects are \"discord\", \"mastodon\", \"matrix\", \"slack\", \"activitypub\"";
    for dialect in ["nowhere", "herald", "herald-push"] {
        let error = loaded(&route(
            "tooted",
            "social",
            Some("long"),
            &format!("{dialect} = \"short\""),
        ))
        .unwrap_err();
        assert!(
            error.contains(&format!(
                "routes[0].templates.{dialect}: no destination kind writes a dialect named \"{dialect}\"; {dialects}"
            )),
            "{error}"
        );
    }
}

#[test]
fn a_template_no_template_names_is_refused_once_with_every_other_mistake() {
    let error = loaded(&route(
        "tooted",
        "social",
        Some("other"),
        "mastodon = \"missing\", discord = \"gone\"",
    ))
    .unwrap_err();
    let named = |name: &str| format!("no template is named \"{name}\"");
    assert_eq!(error.matches(&named("missing")).count(), 1, "{error}");
    assert!(
        error.contains(&format!(
            "routes[0].templates.mastodon: {}",
            named("missing")
        )),
        "{error}"
    );
    assert!(
        error.contains(&format!("routes[0].templates.discord: {}", named("gone"))),
        "{error}"
    );
    assert!(
        error.contains(&format!("routes[0].template: {}", named("other"))),
        "{error}"
    );
    let error = loaded(&route("posted", "chat", Some("long"), "discord = \"gone\"")).unwrap_err();
    assert_eq!(error.matches(&named("gone")).count(), 1, "{error}");
    let error = loaded(&route(
        "posted",
        "chat",
        Some("other"),
        "discord = \"long\"",
    ))
    .unwrap_err();
    assert_eq!(error.matches(&named("other")).count(), 1, "{error}");
}

#[test]
fn a_route_whose_dialect_has_no_template_of_its_own_still_names_one() {
    let error = loaded(&route("posted", "chat", None, "mastodon = \"short\"")).unwrap_err();
    assert!(
        error.contains("routes[0].template: give the template the route renders with"),
        "{error}"
    );
}
