use super::*;

/// A private place beside the harness's public route, whose title names
/// where each event came from.
const TEAM: &str = r#"
[[routes]]
id = "team"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000300"
template = "origin"
private = true
[templates.origin]
title = "{{ event.origin.source }} {{ event.origin.repository }} {{ event.origin.visibility }}"
body = "{{ event.body }}"
"#;

/// The repository, as GitHub answers it, seen by those `visibility` names.
fn repository(visibility: &str) -> String {
    format!(
        r#"{{"id":1,"name":"php-mutation-gate","full_name":"nightworksio/php-mutation-gate","visibility":"{visibility}"}}"#
    )
}

#[test]
fn a_private_repository_s_events_reach_only_the_private_routes_named_by_their_origin() {
    let store = healthy();
    let config = config(TEAM);
    let private = repository("private");
    let one = releases(&["v1"]);
    let first = Script::answering(&[
        (RELEASES, 200, &one),
        ("/repos/nightworksio/php-mutation-gate", 200, &private),
    ]);
    assert!(matches!(
        swept(&config, &store, &first),
        Sweep::Polled { first: true, .. }
    ));
    let two = releases(&["v2", "v1"]);
    let posted = (
        "/channels/1566200000000000300/messages",
        200,
        r#"{"id":"3"}"#,
    );
    let script = Script::answering(&[
        (RELEASES, 200, &two),
        ("/repos/nightworksio/php-mutation-gate", 200, &private),
        posted,
    ]);
    let Sweep::Polled { announced, .. } = swept(&config, &store, &script) else {
        panic!("the source was not polled");
    };
    let routes: Vec<&str> = announced[0]
        .announced
        .iter()
        .map(|done| done.route.as_str())
        .collect();
    assert_eq!(routes, ["team"]);
    let origin = announced[0].event.origin().unwrap();
    assert_eq!(origin.repository, "nightworksio/php-mutation-gate");
    assert_eq!(origin.source, None);
    let body = String::from_utf8(script.bodies()[2].clone()).unwrap();
    assert!(
        body.contains("gate nightworksio/php-mutation-gate private"),
        "{body}"
    );
}
