use std::collections::BTreeMap;

use herald_core::RouteId;
use herald_sources_forge::Override;

use super::*;

/// An app, so a route may name no owner, and a route taking only its events.
const PLUGINS: &str = r#"
[apps.ci]
tokens = ["sha256:1111111111111111111111111111111111111111111111111111111111111111"]
projects = ["deploys"]
[[routes]]
id = "plugins"
sources = ["ci"]
kinds = ["release"]
destination = "discord"
channel = "1566200000000000200"
template = "release"
edits = "release"
"#;

fn routed(routes: &[&str], routes_only: Option<bool>) -> Override {
    Override {
        routes: Some(routes.iter().map(|route| (*route).to_owned()).collect()),
        routes_only,
        ..Override::default()
    }
}

/// The routes each member's events went to in `swept`, by member path.
fn reached(swept: &[(Option<String>, Sweep)]) -> Vec<(String, Vec<RouteId>)> {
    swept
        .iter()
        .filter_map(|(path, sweep)| match sweep {
            Sweep::Polled { announced, .. } => Some((
                path.clone().unwrap_or_default(),
                announced
                    .iter()
                    .flat_map(|fresh| fresh.announced.iter().map(|done| done.route.clone()))
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn a_member_reaches_the_routes_its_override_names_beside_or_in_place_of_its_source_s() {
    let store = healthy();
    let config = owned_in(PLUGINS, |options| {
        options.repositories = BTreeMap::from([
            ("herald".to_owned(), routed(&["plugins"], None)),
            ("gate".to_owned(), routed(&["plugins"], Some(true))),
        ]);
    });
    joined(&store, &config);
    let later = NOON + SignedDuration::from_hours(1);
    let herald = releases("herald", &[("v2", later), ("v1", NOON)]);
    let gate = releases("gate", &[("v2", later), ("v1", NOON)]);
    let (herald_releases, gate_releases) = (released("herald"), released("gate"));
    let (herald_described, gate_described) = (described(1, "herald"), described(2, "gate"));
    let plugged = (
        "/channels/1566200000000000200/messages",
        200,
        r#"{"id":"2"}"#,
    );
    let answers = [
        (LISTING, 304, ""),
        (PROBE, 200, "[]"),
        (&herald_releases[..], 200, &herald[..]),
        ("/repos/nightworksio/herald", 200, &herald_described[..]),
        POSTED,
        CROSSPOSTED,
        THREADED,
        PINNED,
        WHO,
        NO_PINS,
        plugged,
        (&gate_releases[..], 200, &gate[..]),
        ("/repos/nightworksio/gate", 200, &gate_described[..]),
        plugged,
    ];
    let swept = polled(&config, &store, &Script::answering(&answers), later);
    assert_eq!(
        reached(&swept),
        [
            (
                "nightworksio/herald".to_owned(),
                vec![RouteId::new("releases"), RouteId::new("plugins")]
            ),
            (
                "nightworksio/gate".to_owned(),
                vec![RouteId::new("plugins")]
            ),
        ]
    );
    assert!(members(&store).iter().all(|member| {
        member
            .cursor
            .as_ref()
            .is_some_and(|cursor| !cursor.notes.is_empty())
    }));
}
