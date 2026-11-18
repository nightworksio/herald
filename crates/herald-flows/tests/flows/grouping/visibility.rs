//! A group never carries a member to a route its visibility may not reach:
//! the member is refused before it is gathered, and a member gathered under
//! an earlier config that no longer lets it reach the route is left out of
//! the group on every flush (ADR-0029, ADR-0045).

use herald_config::Uses;
use herald_core::{SourceName, Visibility};
use herald_flows::{Budget, Outcome, Ports, announce, prepare};
use herald_ports::Waiting;
use herald_testing::{FakeClock, FakeSigner};

use super::{at, body, gathered, grouped, hold, posted, release, round_outcomes, sent, swept};
use crate::harness::{Fault, POSTED, Script, healthy, secrets};

/// Why a group was announced to no one.
const UNADMITTED: &str = "no member it gathered may reach the route now, as [doctor] private_to_public and the route's private say";

fn private(project: &str, id: u32) -> herald_core::Event {
    release(project, id).seen_as(Visibility::Private)
}

#[test]
fn a_private_member_never_enters_a_group_on_a_route_not_declared_private() {
    let (config, store) = (grouped("", ""), healthy());
    let script = Script::default();
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store: &store,
        clock: &FakeClock::at(at(0)),
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let source = SourceName::new("gate");
    let announced = pollster::block_on(announce(
        &config,
        &connected,
        &ports,
        &source,
        &private("cli", 2),
        "r1",
    ));
    assert_eq!(announced, Vec::new());
    assert_eq!(
        pollster::block_on(store.inner.pending(&hold())).unwrap(),
        None
    );
}

#[test]
fn a_private_member_the_route_no_longer_admits_is_left_out_when_the_public_lead_comes() {
    let store = healthy();
    let script = Script::answering(&[POSTED]);
    let admitting = grouped(", until = \"lead\"", "private = true");
    assert_eq!(
        sent(&admitting, (&store, at(0)), &script, &private("cli", 2)),
        gathered(30)
    );
    let closed = grouped(", until = \"lead\"", "");
    assert_eq!(
        sent(&closed, (&store, at(1)), &script, &release("core", 1)),
        posted()
    );
    let text = body(&script, 0);
    assert!(text.contains("v1.2.0 of 1"), "{text}");
    assert!(text.contains("core with notes"), "{text}");
    assert!(!text.contains("cli"), "{text}");
}

#[test]
fn a_group_whose_every_member_the_route_no_longer_admits_is_announced_to_no_one_once() {
    let store = healthy();
    let script = Script::default();
    let admitting = grouped("", "private = true");
    assert_eq!(
        sent(&admitting, (&store, at(0)), &script, &private("cli", 2)),
        gathered(30)
    );
    let closed = grouped("", "");
    assert_eq!(
        round_outcomes(&swept(&closed, (&store, at(30)), &script)),
        [Outcome::Skipped(UNADMITTED.to_owned())]
    );
    assert_eq!(script.seen(), Vec::<String>::new());
    let kept = pollster::block_on(store.inner.pending(&hold()))
        .unwrap()
        .unwrap();
    assert!(kept.value.landed);
    assert_eq!(swept(&closed, (&store, at(31)), &script), Vec::new());
}

#[test]
fn a_group_announced_to_no_one_whose_record_cannot_be_read_stops() {
    let mut store = healthy();
    let script = Script::default();
    let admitting = grouped("", "private = true");
    assert_eq!(
        sent(&admitting, (&store, at(0)), &script, &private("cli", 2)),
        gathered(30)
    );
    store.fault = Fault::ReadsHeld;
    let outcomes = round_outcomes(&swept(&grouped("", ""), (&store, at(30)), &script));
    assert!(
        matches!(outcomes[..], [Outcome::Stopped { .. }]),
        "{outcomes:?}"
    );
}

#[test]
fn a_private_member_the_route_still_admits_is_announced_with_the_group() {
    let store = healthy();
    let script = Script::answering(&[POSTED]);
    let admitting = grouped("", "private = true");
    assert_eq!(
        sent(&admitting, (&store, at(0)), &script, &private("cli", 2)),
        gathered(30)
    );
    assert_eq!(
        sent(&admitting, (&store, at(1)), &script, &release("core", 1)),
        gathered(30)
    );
    assert_eq!(
        round_outcomes(&swept(&admitting, (&store, at(30)), &script)),
        [posted()]
    );
    let text = body(&script, 0);
    assert!(text.contains("v1.2.0 of 2"), "{text}");
}
