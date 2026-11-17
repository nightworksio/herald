use std::sync::Arc;

use herald_sources_registry::{OciOptions, OciSource};
use serde_json::json;

use super::packages::{DEDUPED, Keyed, PACKAGES, VERSIONS, tagged};
use super::*;

const CATALOG: &str = "/v2/_catalog?n=1000";

const TAGS: &str = "/v2/acme/app/tags/list?n=1000";

fn catalogued() -> Config {
    let mut config = config("");
    config.sources.insert(
        SourceName::new("gate"),
        Source::new(OciSource::new(OciOptions {
            registry: "https://registry.example".to_owned(),
            catalog: true,
            ..OciOptions::default()
        })),
    );
    config
}

fn annotated(created: &str) -> String {
    json!({
        "annotations": {"org.opencontainers.image.created": created},
        "config": {"digest": "sha256:c0ffee"}
    })
    .to_string()
}

#[test]
fn a_catalog_s_repositories_record_their_tags_first_and_announce_each_new_one_after() {
    let store = healthy();
    let config = catalogued();
    let repositories = json!({"repositories": ["acme/app"]}).to_string();
    let opening = Script::answering(&[
        (CATALOG, 200, &repositories),
        (TAGS, 200, r#"{"tags":["1.0"]}"#),
    ]);
    assert_eq!(
        polled(&config, &store, &opening, NOON),
        [
            (
                None,
                Sweep::Listed {
                    members: 1,
                    joined: vec!["acme/app".to_owned()],
                    left: Vec::new(),
                }
            ),
            (Some("acme/app".to_owned()), first(0)),
        ]
    );
    let mut answers = vec![
        (CATALOG, 200, repositories.as_str()),
        (TAGS, 200, r#"{"tags":["1.0","2.0"]}"#),
    ];
    let created = annotated("2026-11-01T00:00:00Z");
    answers.push(("/v2/acme/app/manifests/2.0", 200, created.as_str()));
    answers.extend(crate::poll::announced());
    let later = Script::answering(&answers);
    let swept = polled(
        &config,
        &store,
        &later,
        NOON + SignedDuration::from_hours(1),
    );
    assert!(
        matches!(
            &swept[1],
            (Some(path), Sweep::Polled { first: false, found: 1, announced, .. })
                if path == "acme/app" && announced.len() == 1
        ),
        "{swept:?}"
    );
    let quiet = Script::answering(&[
        (CATALOG, 200, &repositories),
        (TAGS, 200, r#"{"tags":["1.0","2.0"]}"#),
    ]);
    let swept = polled(
        &config,
        &store,
        &quiet,
        NOON + SignedDuration::from_hours(2),
    );
    assert!(
        matches!(&swept[1], (_, Sweep::Polled { found: 0, .. })),
        "{swept:?}"
    );
}

/// What `config` posts, polled once through `answers` keyed by each
/// request's line, with every source's token read.
fn posted(config: &Config, answers: Vec<(&str, u16, String)>) -> usize {
    let store = healthy();
    let mut answers: Vec<(String, u16, String)> = answers
        .into_iter()
        .map(|(line, status, body)| (line.to_owned(), status, body))
        .collect();
    answers.extend(
        crate::poll::announced()
            .into_iter()
            .map(|(line, status, body)| (line.to_owned(), status, body.to_owned())),
    );
    let script = Keyed {
        answers,
        seen: Arc::default(),
    };
    let tokened = secrets().with("GITHUB_TOKEN", "github-token");
    let prepared = pollster::block_on(prepare(config, &tokened, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = herald_flows::Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let swept = pollster::block_on(poll(config, &connected, &ports, "r1"));
    assert!(
        swept
            .iter()
            .all(|swept| !matches!(swept.outcome, Sweep::Stopped(_))),
        "{swept:?}"
    );
    let seen = script.seen.lock().unwrap().clone();
    seen.iter()
        .filter(|line| line.starts_with("POST") && line.ends_with("/messages"))
        .count()
}

fn loaded(text: &str) -> Config {
    herald_config::load(
        "herald.toml",
        text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

#[test]
fn an_image_docker_hub_and_an_oci_source_on_docker_io_both_see_is_announced_once() {
    let config = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[run]
sources = 1
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.hub]
kind = "docker-hub"
package = "caddy"
backfill = 1
[sources.registry]
kind = "oci"
registry = "https://registry-1.docker.io"
repository = "caddy"
backfill = 1
[[routes]]
id = "releases"
sources = ["hub", "registry"]
destination = "discord"
channel = "1566200000000000100"
template = "release"
[templates.release]
title = "{{ event.project }} {{ event.attributes.version }}"
body = "{{ event.body }}"
"#,
    );
    let pushed =
        json!({"results": [{"name": "2.10.0", "tag_last_pushed": NOON.to_string()}]}).to_string();
    let answers = vec![
        (
            "/namespaces/library/repositories/caddy/tags?page_size=30&ordering=last_updated",
            200,
            pushed,
        ),
        (
            "/v2/library/caddy/tags/list?n=1000",
            200,
            r#"{"tags":["2.10.0"]}"#.to_owned(),
        ),
        (
            "/v2/library/caddy/manifests/2.10.0",
            200,
            annotated(&NOON.to_string()),
        ),
    ];
    assert_eq!(posted(&config, answers), 1);
}

#[test]
fn an_image_a_repository_s_package_signal_and_an_oci_source_on_ghcr_both_see_is_announced_once() {
    let config = loaded(&DEDUPED.replace(
        "kind = \"ghcr\"\nowner = \"nightworksio\"\ntoken = { secret = \"GITHUB_TOKEN\" }",
        "kind = \"oci\"\nregistry = \"https://ghcr.io\"\nrepository = \"nightworksio/gate-image\"",
    ));
    let described = json!({
        "id": 5,
        "name": "php-mutation-gate",
        "html_url": "https://github.com/nightworksio/php-mutation-gate",
        "visibility": "public",
        "private": false,
        "owner": {"login": "nightworksio", "type": "Organization"}
    })
    .to_string();
    let listing = json!([{
        "id": 77,
        "name": "gate-image",
        "html_url": "https://github.com/orgs/nightworksio/packages/container/package/gate-image",
        "visibility": "public",
        "repository": {"id": 5, "name": "php-mutation-gate", "full_name": "nightworksio/php-mutation-gate", "visibility": "public"}
    }])
    .to_string();
    let answers = vec![
        (PACKAGES, 200, listing),
        ("/repos/nightworksio/php-mutation-gate", 200, described),
        (VERSIONS, 200, tagged("v1")),
        (
            "/v2/nightworksio/gate-image/tags/list?n=1000",
            200,
            r#"{"tags":["v1"]}"#.to_owned(),
        ),
        (
            "/v2/nightworksio/gate-image/manifests/v1",
            200,
            annotated(&NOON.to_string()),
        ),
    ];
    assert_eq!(posted(&config, answers), 1);
}

#[test]
fn an_oci_source_whose_registry_sends_its_tokens_from_an_untrusted_realm_is_a_gap_online() {
    let mut config = config("");
    config.sources.insert(
        herald_core::SourceName::new("gate"),
        herald_config::Source::new(herald_sources_registry::OciSource::new(
            herald_sources_registry::OciOptions {
                registry: "https://registry.example".to_owned(),
                repository: Some("acme/app".to_owned()),
                ..herald_sources_registry::OciOptions::default()
            },
        )),
    );
    let script = Script::answering(&[
        ("/users/@me", 200, r#"{"id":"99"}"#),
        (
            "/channels/1566200000000000100",
            200,
            r#"{"id":"1566200000000000100","guild_id":"1","type":5}"#,
        ),
        ("/v2/acme/app/tags/list?n=1", 401, ""),
    ])
    .headed(&[(
        "WWW-Authenticate",
        r#"Bearer realm="https://elsewhere.example/token""#,
    )]);
    let prepared = pollster::block_on(prepare(
        &config,
        &crate::harness::secrets(),
        &FakeSigner,
        Uses::Polls,
    ))
    .unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let findings = pollster::block_on(herald_flows::online(&connected, (&FakeSigner, NOON)));
    assert_eq!(
        findings.gaps,
        [herald_flows::Gap {
            feature: "reading the source \"gate\"".to_owned(),
            key: "sources.gate.auth".to_owned(),
            why: "the registry sends its tokens from https://elsewhere.example, which is neither its own origin nor auth's; set auth to that service's base URL where it is the registry's own".to_owned(),
            first_layer: None,
        }]
    );
}
