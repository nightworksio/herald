//! A push to a peer: proved and signed by `[federation]`'s keys on its
//! domain, joined to a peer of `[peers]`, recorded in a store that keeps
//! records, reached by routes that render no template, and by no route
//! declared private unless the peer takes private events.

use herald_config::{Config, Uses, load};
use herald_core::SecretRef;
use herald_peers::multibase::ed25519_public;
use herald_ports::Secret;

const FEDERATION: &str = r#"
[federation]
domain = "herald.example"
keys = [{ secret = "HERALD_KEY_NEW" }, { secret = "HERALD_KEY_OLD" }]
"#;

const PUSH: &str = r#"
[destinations.mirror]
kind = "herald-push"
peer = "upstream"
inbox = "releases"
"#;

fn peer(extra: &str) -> String {
    format!(
        "[peers.upstream]\nurl = \"https://peer.example/h\"\nkeys = [\"{}\"]\n{extra}\n",
        ed25519_public(&[5; 32])
    )
}

fn route(extra: &str) -> String {
    format!(
        r#"
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "pushed"
sources = ["gate"]
destination = "mirror"
{extra}
"#
    )
}

fn loaded(store: &str, head: &str, route: &str) -> Result<Config, String> {
    let text = format!("version = 1\n\n[store]\n{store}\n{head}{route}");
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::common::kinds(),
    )
    .map_err(|errors| errors.to_string())
}

fn sqlite(head: &str, extra: &str) -> Result<Config, String> {
    loaded("kind = \"sqlite\"", head, &route(extra))
}

fn pushed(config: &Config) -> (String, bool) {
    let destination = config.destinations.values().next().unwrap();
    let reached = destination
        .reach(
            &config.routes[0].options,
            "",
            Secret::new("HERALD_KEY_NEW", "k"),
        )
        .unwrap();
    let pushed = reached.pushes().unwrap();
    (pushed.inbox.to_owned(), pushed.private)
}

#[test]
fn a_push_is_signed_by_the_instance_s_newest_key_to_the_peer_it_names() {
    let config = sqlite(&format!("{FEDERATION}{}{PUSH}", peer("")), "").unwrap();
    assert_eq!(config.routes[0].route.template, None);
    let destination = config.destinations.values().next().unwrap();
    assert_eq!(
        config.credential(destination),
        Some(&SecretRef {
            secret: "HERALD_KEY_NEW".to_owned(),
        })
    );
    assert!(
        config
            .secrets_used(Uses::Announces)
            .contains("HERALD_KEY_NEW")
    );
    assert_eq!(
        pushed(&config),
        (
            "https://peer.example/h/herald/v1/inbox/releases".to_owned(),
            false
        )
    );
}

#[test]
fn a_push_needs_the_federation_s_keys_a_peer_and_a_store_that_keeps_records() {
    for federation in ["", "[federation]\ndomain = \"herald.example\"\n"] {
        let unkeyed = sqlite(&format!("{federation}{}{PUSH}", peer("")), "").unwrap_err();
        assert!(
                unkeyed.contains("destinations.mirror: a push to a peer is proved and signed by [federation]'s keys on its domain; give domain and keys in [federation]"),
                "{unkeyed}"
            );
    }
    let unpeered = sqlite(&format!("{FEDERATION}{PUSH}"), "").unwrap_err();
    assert!(
            unpeered.contains("destinations.mirror.peer: there is no peer \"upstream\" in [peers]; `herald peer add <url>` pins one"),
            "{unpeered}"
        );
    let markers = loaded(
        "kind = \"markers\"",
        &format!("{FEDERATION}{}{PUSH}", peer("")),
        &route(""),
    )
    .unwrap_err();
    assert!(
            markers.contains("destinations.mirror: the markers store keeps what was announced in the posts it reads back, and a push reads none; give a store that keeps records, such as \"git-branch\""),
            "{markers}"
        );
    let templated = sqlite(
        &format!("{FEDERATION}{}{PUSH}", peer("")),
        "template = \"release\"",
    )
    .unwrap_err();
    assert!(
            templated.contains("routes[0].template: a herald feed or push carries each event as an envelope and renders no template; remove template"),
            "{templated}"
        );
}

#[test]
fn a_route_declared_private_pushes_only_to_a_peer_taking_private_events() {
    let closed = sqlite(&format!("{FEDERATION}{}{PUSH}", peer("")), "private = true").unwrap_err();
    assert!(
            closed.contains("routes[0].private: this route is declared private, and its push goes to a peer that takes no private events"),
            "{closed}"
        );
    let open = sqlite(
        &format!("{FEDERATION}{}{PUSH}", peer("private = true")),
        "private = true",
    )
    .unwrap();
    assert!(pushed(&open).1);
    let warned = sqlite(
        &format!(
            "{FEDERATION}{}{PUSH}[doctor]\nprivate_to_public = \"warn\"\n",
            peer("")
        ),
        "private = true",
    );
    assert!(warned.is_ok(), "{warned:?}");
}
