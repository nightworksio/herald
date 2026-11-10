use std::collections::BTreeMap;

use jiff::Timestamp;

use super::*;
use crate::event::Draft;

fn event(version: &str, prerelease: bool, at: i64) -> Event {
    Event::normalise(Draft {
        identity: format!("release/{version}"),
        kind: Kind::RELEASE,
        project: "herald".to_owned(),
        attributes: crate::versioned(version, prerelease),
        title: format!("herald {version}"),
        body: "Notes".to_owned(),
        links: BTreeMap::from([(
            "release".to_owned(),
            format!("https://example.org/{version}"),
        )]),
        occurred: Timestamp::from_second(at).unwrap(),
    })
    .unwrap()
}

fn route(id: &str) -> RouteId {
    RouteId::new(id)
}

#[test]
fn the_first_event_is_kept_as_newest_and_stable_with_its_route() {
    let latest = Latest::after(None, &event("1.0.0", false, 10), &route("deploys"));
    assert_eq!(latest.project, "herald");
    assert_eq!(latest.newest.version(), Some("1.0.0"));
    assert_eq!(latest.newest.title, "herald 1.0.0");
    assert_eq!(latest.newest.body, "Notes");
    assert_eq!(latest.newest.links["release"], "https://example.org/1.0.0");
    assert_eq!(latest.newest.kind, Kind::RELEASE);
    assert_eq!(latest.stable, Some(latest.newest.clone()));
    assert_eq!(latest.routes, BTreeSet::from([route("deploys")]));
}

#[test]
fn a_first_pre_release_is_newest_and_leaves_no_stable() {
    let latest = Latest::after(None, &event("1.0.0-rc.1", true, 10), &route("r"));
    assert!(latest.newest.prerelease());
    assert_eq!(latest.stable, None);
    assert_eq!(latest.shown(false).version(), Some("1.0.0-rc.1"));
}

#[test]
fn a_newer_pre_release_becomes_newest_and_keeps_the_stable_one() {
    let kept = Latest::after(None, &event("1.0.0", false, 10), &route("r"));
    let latest = Latest::after(Some(kept), &event("1.1.0-rc.1", true, 20), &route("r"));
    assert_eq!(latest.shown(true).version(), Some("1.1.0-rc.1"));
    assert_eq!(latest.shown(false).version(), Some("1.0.0"));
}

#[test]
fn an_older_event_leaves_what_is_kept_and_adds_its_route() {
    let kept = Latest::after(None, &event("2.0.0", false, 20), &route("one"));
    let latest = Latest::after(Some(kept), &event("1.0.0", false, 10), &route("two"));
    assert_eq!(latest.newest.version(), Some("2.0.0"));
    assert_eq!(
        latest.stable.as_ref().and_then(|s| s.version()),
        Some("2.0.0")
    );
    assert_eq!(latest.routes, BTreeSet::from([route("one"), route("two")]));
}

#[test]
fn an_event_at_the_same_instant_replaces_what_is_kept() {
    let kept = Latest::after(None, &event("1.0.0", false, 10), &route("r"));
    let latest = Latest::after(Some(kept), &event("1.0.1", false, 10), &route("r"));
    assert_eq!(latest.newest.version(), Some("1.0.1"));
    assert_eq!(
        latest.stable.as_ref().and_then(|s| s.version()),
        Some("1.0.1")
    );
}

#[test]
fn a_stable_event_after_only_pre_releases_becomes_stable() {
    let kept = Latest::after(None, &event("1.0.0-rc.1", true, 10), &route("r"));
    let latest = Latest::after(Some(kept), &event("0.9.0", false, 5), &route("r"));
    assert_eq!(latest.newest.version(), Some("1.0.0-rc.1"));
    assert_eq!(latest.shown(false).version(), Some("0.9.0"));
}
