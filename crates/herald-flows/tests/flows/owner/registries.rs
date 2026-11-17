use herald_config::{Config, Source, Uses};
use herald_core::SourceName;
use herald_flows::{Sweep, prepare};
use herald_sources_registry::{Registry, RegistryOptions, RegistrySource};
use herald_testing::FakeSigner;
use serde_json::json;

use super::{first, polled};
use crate::harness::*;

fn namespaced(registry: Registry, options: RegistryOptions) -> Config {
    let mut config = config("");
    config.sources.insert(
        SourceName::new("gate"),
        Source::new(RegistrySource { registry, options }),
    );
    config
}

const LISTING: &str = "/namespaces/acme/repositories?page_size=100&page=1";

fn tags(repository: &str) -> String {
    format!("/namespaces/acme/repositories/{repository}/tags?page_size=30&ordering=last_updated")
}

#[test]
fn a_registry_namespace_is_listed_and_each_package_polled_as_an_owner_s_members_are() {
    let store = healthy();
    let config = namespaced(
        Registry::DockerHub,
        RegistryOptions {
            namespace: Some("acme".to_owned()),
            exclude: vec!["old".to_owned()],
            ..RegistryOptions::default()
        },
    );
    let listing = json!({"results": [
        {"name": "api", "namespace": "acme", "is_private": false},
        {"name": "old", "namespace": "acme", "is_private": false},
    ], "next": null})
    .to_string();
    let pushed = json!({"results": [{"name": "1.0", "tag_last_pushed": "2026-10-31T12:00:00Z"}]})
        .to_string();
    let script = Script::answering(&[(LISTING, 200, &listing), (&tags("api"), 200, &pushed)]);
    assert_eq!(
        polled(&config, &store, &script, NOON),
        [
            (
                None,
                Sweep::Listed {
                    members: 1,
                    joined: vec!["acme/api".to_owned()],
                    left: Vec::new(),
                }
            ),
            (Some("acme/api".to_owned()), first(1)),
        ]
    );
    let refused = Script::answering(&[(LISTING, 500, "{}")]);
    assert!(matches!(
        polled(&config, &store, &refused, NOON)[0],
        (None, Sweep::Stopped(_))
    ));
    let unchanged = Script::answering(&[(LISTING, 200, &listing), (&tags("api"), 304, "")]);
    assert_eq!(
        polled(&config, &store, &unchanged, NOON)[1],
        (Some("acme/api".to_owned()), Sweep::Unchanged)
    );
}

#[test]
fn a_packagist_vendor_reaches_its_listing_api_beside_its_registry_through_one_connection() {
    let mut config = namespaced(
        Registry::Packagist,
        RegistryOptions {
            vendor: Some("acme".to_owned()),
            ..RegistryOptions::default()
        },
    );
    config.sources.insert(
        SourceName::new("again"),
        Source::new(RegistrySource {
            registry: Registry::Packagist,
            options: RegistryOptions {
                vendor: Some("other".to_owned()),
                ..RegistryOptions::default()
            },
        }),
    );
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let mut bases = Vec::new();
    prepared
        .connect(|base| {
            bases.push(base.to_string());
            Ok::<_, ()>(())
        })
        .unwrap();
    assert_eq!(
        bases
            .iter()
            .filter(|base| *base == "https://repo.packagist.org, https://packagist.org")
            .count(),
        1
    );
    let store = healthy();
    let script = Script::answering(&[
        (
            "https://packagist.org/packages/list.json?vendor=other",
            200,
            r#"{"packageNames":[]}"#,
        ),
        (
            "https://packagist.org/packages/list.json?vendor=acme",
            200,
            r#"{"packageNames":[]}"#,
        ),
    ]);
    let swept = super::polled_within(&config, &store, &script, NOON, None);
    assert_eq!(swept.len(), 2);
}
