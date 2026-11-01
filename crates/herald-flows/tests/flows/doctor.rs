use herald_config::load;
use herald_flows::{Gap, doctor, unserved};

use crate::harness::*;

const STORES: [&str; 2] = ["git-branch", "markers"];

fn on(store: &str, extra: &str) -> herald_config::Config {
    let mut config = config(extra);
    config.store = load(
        "herald.toml",
        &format!("version = 1\n[store]\nkind = \"{store}\"\n"),
        &|_| Err(String::new()),
    )
    .unwrap()
    .store;
    config
}

fn keys(gaps: &[Gap]) -> Vec<&str> {
    gaps.iter().map(|gap| gap.key.as_str()).collect()
}

#[test]
fn a_config_on_a_store_that_keeps_records_has_no_gaps_and_notes_a_derived_marker_key() {
    let findings = doctor(&on("git-branch", ""), &STORES);
    assert_eq!(findings.gaps, []);
    assert_eq!(findings.notes.len(), 1);
    assert!(findings.notes[0].contains("Rotating that credential forgets every earlier marker"));
    let keyed = on(
        "git-branch",
        "[markers]\nkey = { secret = \"HERALD_MARKER_SECRET\" }\n",
    );
    assert_eq!(doctor(&keyed, &STORES).notes, Vec::<String>::new());
}

#[test]
fn the_actions_cache_and_repository_variables_note_what_they_cannot_hold() {
    const ALL: [&str; 4] = ["git-branch", "markers", "actions-cache", "repo-variables"];
    let cache = doctor(&on("actions-cache", ""), &ALL);
    assert_eq!(cache.gaps, []);
    assert!(cache.notes[1].starts_with("The Actions cache keeps records apart for each ref"));
    let mut variables = on("git-branch", "");
    variables.store = load(
        "herald.toml",
        "version = 1\n[store]\nkind = \"repo-variables\"\ntoken = { secret = \"T\" }\n",
        &|_| Err(String::new()),
    )
    .unwrap()
    .store;
    let noted = doctor(&variables, &ALL);
    assert!(noted.notes[1].starts_with("Each record is a repository variable"));
}

#[test]
fn a_store_this_herald_does_not_serve_is_a_gap() {
    let findings = doctor(&on("sqlite", ""), &STORES);
    assert_eq!(
        findings.gaps,
        [Gap {
            feature: "keeping records in sqlite".to_owned(),
            key: "store.kind".to_owned(),
            why: unserved(&STORES),
        }]
    );
    assert_eq!(
        unserved(&STORES),
        "this herald keeps records in git-branch or markers; set [store] kind to one of them"
    );
    assert_eq!(
        unserved(&["a", "b", "c"]),
        "this herald keeps records in a, b or c; set [store] kind to one of them"
    );
    assert_eq!(
        unserved(&["a"]),
        "this herald keeps records in a; set [store] kind to one of them"
    );
}

#[test]
fn the_markers_store_names_the_pins_cursors_and_read_backs_it_lacks() {
    let mut config = on("markers", "");
    config.routes[0].route.delivery.pin = Some(herald_core::Pinning::Store);
    let findings = doctor(&config, &STORES);
    assert_eq!(keys(&findings.gaps), ["routes[0].pin", "sources.gate"]);
    assert_eq!(
        findings.gaps[1].why,
        "the markers store keeps no cursors; `herald poll` needs a store that does"
    );
    assert!(findings.notes[1].starts_with("The markers store has no compare-and-set"));
    config.routes[0].route.delivery.pin = Some(herald_core::Pinning::Markers);
    let herald_config::RouteOptions::Discord(placed) = &mut config.routes[0].options;
    placed.forum = true;
    let forum = doctor(&config, &STORES);
    assert_eq!(keys(&forum.gaps), ["routes[0].destination", "sources.gate"]);
    assert!(
        forum.gaps[0]
            .why
            .contains("a forum's posts are not read back")
    );
    let mut hooked = on("markers", "");
    if let Some(herald_config::Destination::Discord(options)) =
        hooked.destinations.values_mut().next()
    {
        options.webhook = options.token.take();
    }
    hooked.routes[0].route.delivery.pin = None;
    hooked.routes[0].route.delivery.crosspost = false;
    hooked.routes[0].route.delivery.thread = None;
    let webhook = doctor(&hooked, &STORES);
    assert!(
        webhook.gaps[0]
            .why
            .contains("a webhook cannot read back its posts")
    );
    assert_eq!(webhook.gaps[0].feature, "sending each announcement once");
}
