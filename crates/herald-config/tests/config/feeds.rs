//! A feed of herald's own: served on `[federation]`'s domain and proved by
//! its keys, kept in a store that keeps records, fitted to the most a
//! record holds, and reached by routes that render no template.

use herald_config::{Config, Uses, load};
use herald_core::SecretRef;
use herald_kinds::{Bound, Feeding};
use jiff::SignedDuration;

const FEDERATION: &str = r#"
[federation]
domain = "herald.example"
keys = [{ secret = "HERALD_KEY_NEW" }, { secret = "HERALD_KEY_OLD" }]
"#;

const FEED: &str = r#"
[destinations.releases]
kind = "herald"
"#;

const ROUTE: &str = r#"
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "fed"
sources = ["gate"]
destination = "releases"
"#;

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn loaded(store: &str, head: &str, route: &str) -> Result<Config, String> {
    let text = format!("version = 1\n\n[store]\n{store}\n{head}{route}");
    load("herald.toml", &text, &nothing, &crate::common::kinds())
        .map_err(|errors| errors.to_string())
}

fn sqlite(head: &str) -> Result<Config, String> {
    loaded("kind = \"sqlite\"", head, ROUTE)
}

fn feeding(config: &Config) -> Feeding {
    config
        .destinations
        .values()
        .find_map(|destination| destination.feeding())
        .unwrap()
}

fn secret(name: &str) -> SecretRef {
    SecretRef {
        secret: name.to_owned(),
    }
}

#[test]
fn a_feed_is_proved_by_the_instance_s_newest_key_and_its_route_renders_no_template() {
    let config = sqlite(&format!("{FEDERATION}{FEED}")).unwrap();
    assert_eq!(
        feeding(&config),
        Feeding {
            keep: 1000,
            page: 20,
            envelope: 16 * 1024,
            bound: Bound::Default,
        }
    );
    assert_eq!(config.routes[0].route.template, None);
    let destination = config.destinations.values().next().unwrap();
    assert_eq!(
        config.credential(destination),
        Some(&secret("HERALD_KEY_NEW"))
    );
    for uses in [Uses::Announces, Uses::Polls] {
        let used = config.secrets_used(uses);
        assert!(used.contains("HERALD_KEY_NEW"), "{uses:?}");
        assert!(!used.contains("HERALD_KEY_OLD"), "{uses:?}");
    }
    assert!(config.secrets_used(Uses::Serves).contains("HERALD_KEY_OLD"));
}

#[test]
fn a_feed_without_the_instance_s_domain_and_keys_does_not_load() {
    let refused = "destinations.releases: a herald feed is served on [federation]'s domain and its envelopes proved by its keys; give domain and keys in [federation]";
    for head in [
        FEED.to_owned(),
        format!("[federation]\ndomain = \"herald.example\"\n{FEED}"),
    ] {
        let error = sqlite(&head).unwrap_err();
        assert!(error.contains(refused), "{error}");
    }
}

#[test]
fn a_route_to_a_feed_naming_a_template_does_not_load() {
    let templated =
        format!("{ROUTE}template = \"t\"\n\n[templates.t]\ntitle = \"t\"\nbody = \"b\"\n");
    let error = loaded(
        "kind = \"sqlite\"",
        &format!("{FEDERATION}{FEED}"),
        &templated,
    )
    .unwrap_err();
    assert!(
            error.contains("routes[0].template: a herald feed or push carries each event as an envelope and renders no template; remove template"),
            "{error}"
        );
}

#[test]
fn a_feed_beside_a_store_that_does_not_load_names_only_the_store() {
    let error = loaded("kind = \"nowhere\"", &format!("{FEDERATION}{FEED}"), ROUTE).unwrap_err();
    assert!(error.contains("store.kind"), "{error}");
    assert!(!error.contains("destinations.releases"), "{error}");
}

#[test]
fn the_markers_store_keeps_no_feed() {
    let error = loaded("kind = \"markers\"", &format!("{FEDERATION}{FEED}"), ROUTE).unwrap_err();
    assert!(
            error.contains("destinations.releases: the markers store keeps no feed, and a herald feed is kept as a record; give a store that keeps records, such as \"git-branch\""),
            "{error}"
        );
}

#[test]
fn a_feed_naming_no_bound_is_fitted_to_a_store_keeping_small_records() {
    let config = loaded("kind = \"deno-kv\"", &format!("{FEDERATION}{FEED}"), ROUTE).unwrap();
    assert_eq!(
        feeding(&config),
        Feeding {
            keep: 3,
            page: 20,
            envelope: 16 * 1024,
            bound: Bound::Fitted,
        }
    );
    let error = loaded(
        "kind = \"deno-kv\"",
        &format!("{FEDERATION}{FEED}keep = 10\n"),
        ROUTE,
    )
    .unwrap_err();
    assert!(
            error.contains("destinations.releases.keep: the deno-kv store keeps at most 65536 bytes in a record, and this feed may take 168576 keeping 10 envelopes; make keep at most 3 or envelope smaller, or give a store that keeps more"),
            "{error}"
        );
    let error = loaded(
        "kind = \"deno-kv\"",
        &format!("{FEDERATION}{FEED}envelope = \"64KiB\"\n"),
        ROUTE,
    )
    .unwrap_err();
    assert!(
        error.contains("make keep at most 0 or envelope smaller"),
        "{error}"
    );
    let fits = loaded(
        "kind = \"deno-kv\"",
        &format!("{FEDERATION}{FEED}keep = 3\n"),
        ROUTE,
    )
    .unwrap();
    assert_eq!(feeding(&fits).bound, Bound::Named);
}

#[test]
fn a_private_route_to_a_feed_does_not_load_unless_doctor_warns() {
    let private = format!("{ROUTE}private = true\n");
    let error = loaded(
        "kind = \"sqlite\"",
        &format!("{FEDERATION}{FEED}"),
        &private,
    )
    .unwrap_err();
    assert!(
            error.contains("routes[0].private: this route is declared private, and a herald feed is served to anyone and takes only public events; route private events elsewhere, or set [doctor] private_to_public = \"warn\""),
            "{error}"
        );
    let warned = format!("[doctor]\nprivate_to_public = \"warn\"\n{FEDERATION}{FEED}");
    assert!(loaded("kind = \"sqlite\"", &warned, &private).is_ok());
}

#[test]
fn a_response_stays_signed_five_minutes_and_twenty_at_most() {
    let config = sqlite(&format!("{FEDERATION}{FEED}")).unwrap();
    assert_eq!(config.federation.signed_for, SignedDuration::from_mins(5));
    let set = sqlite(&format!("{FEDERATION}signed_for = \"20m\"\n{FEED}")).unwrap();
    assert_eq!(set.federation.signed_for, SignedDuration::from_mins(20));
    let error = sqlite(&format!("{FEDERATION}signed_for = \"21m\"\n{FEED}")).unwrap_err();
    assert!(
        error.contains("federation.signed_for: signed_for is at most \"20m\"; shorten it"),
        "{error}"
    );
}
