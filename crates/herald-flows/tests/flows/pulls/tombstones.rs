//! A tombstone another herald's feed carries, once proved, withdrawing
//! what each route taking the source announced of its identity, as the
//! route's `withdraw` says.

use herald_config::Uses;
use herald_core::{DestinationName, Identity, RouteId, SourceName, Step, Tombstone, Withdraw};
use herald_flows::{Announced, Outcome, Ports, Remembered, Stop, Sweep, announce, prepare};
use herald_peers::keys::method;
use herald_ports::Published as _;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use jiff::SignedDuration;
use serde_json::{Value, json};

use super::{
    DISCOVERY, DOMAIN, FROM_PEER, NEW, OLD, Peer, announced, config, cursor, envelope, page,
    proved, proved_at, pulled, release, rounded, routed, swept,
};
use crate::harness::{Failing, Fault, NOON, Script, event, healthy, outcome};
use crate::withdraw::{KEPT, UNANNOUNCED};

/// The peer's tombstone withdrawing its release `identity`, proved by
/// `key` a minute after its release was.
pub(crate) fn tombstone(identity: &str, key: &[u8; 32]) -> Value {
    proved_at(
        &json!({
            "type": "herald:Tombstone",
            "identity": identity,
            "deleted": "2026-11-17T11:00:00Z",
            "path": [DISCOVERY],
        }),
        (DOMAIN, key),
        NOON + SignedDuration::from_mins(1),
    )
}

/// `config`, each of its routes deleting what the forge or a feed
/// withdraws.
fn deleting(mut config: herald_config::Config) -> herald_config::Config {
    for route in &mut config.routes {
        route.route.delivery.withdraw = Withdraw::Delete;
    }
    config
}

/// A store whose source announced the peer's release `a/2`.
fn announcing(config: &herald_config::Config) -> Failing {
    let store = healthy();
    pulled(
        config,
        &store,
        &Peer::answering(vec![page(&[(1, release("a/1", &OLD))], &OLD)]),
    );
    let later = Peer::answering(vec![page(
        &[(2, release("a/2", &OLD)), (1, release("a/1", &OLD))],
        &OLD,
    )]);
    assert_eq!(
        announced(&pulled(config, &store, &later)),
        ["herald/upstream/a/2"]
    );
    store
}

/// What the route did with the identity a pull's tombstone withdrew, with
/// the pull's own sweep.
fn withdrawn(sweeps: &[Sweep]) -> (&Sweep, &Identity, &Outcome) {
    let [
        polled,
        Sweep::Withdrawn {
            identity,
            announced,
        },
    ] = sweeps
    else {
        panic!("{sweeps:?}");
    };
    let [Announced { route, outcome }] = announced.as_slice() else {
        panic!("{announced:?}");
    };
    assert_eq!(*route, RouteId::new("mirror"));
    (polled, identity, outcome)
}

#[test]
fn a_proved_tombstone_withdraws_the_post_the_route_announced_of_it() {
    let config = deleting(config("", FROM_PEER));
    let store = announcing(&config);
    let tombstoned = Peer::answering(vec![page(
        &[(3, tombstone("a/2", &OLD)), (2, release("a/2", &OLD))],
        &OLD,
    )]);
    let sweeps = swept(&config, &store, &tombstoned);
    let (polled, identity, outcome) = withdrawn(&sweeps);
    assert_eq!(
        *polled,
        Sweep::Polled {
            first: false,
            found: 0,
            announced: Vec::new(),
            cursor: Remembered::Advanced,
        }
    );
    assert_eq!(identity.as_str(), "herald/upstream/a/2");
    assert_eq!(
        *outcome,
        Outcome::Sent {
            steps: vec![Step::Delete(0)]
        }
    );
    let feed = pollster::block_on(store.inner.feed(&DestinationName::new("mirror")))
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(feed.items[0].envelope["type"], "herald:Tombstone");
    assert_eq!(feed.items[0].envelope["identity"], "herald/upstream/a/2");
    let kept = cursor(&store).unwrap();
    assert_eq!(kept.sequence, Some(3));
    assert!(!kept.seen.contains(identity));
    assert!(
        kept.proof(identity)
            .is_some_and(|proof| proof.tombstone == Some(Tombstone::Held))
    );
}

#[test]
fn an_instance_at_its_share_is_refused_by_name_while_its_tombstones_still_withdraw() {
    let config = deleting(config("", &format!("{FROM_PEER}\npeer_share = 1")));
    let store = announcing(&config);
    let first = Peer::answering(vec![page(&[(3, tombstone("a/2", &OLD))], &OLD)]);
    let sweeps = swept(&config, &store, &first);
    let (_, identity, outcome) = withdrawn(&sweeps);
    assert_eq!(identity.as_str(), "herald/upstream/a/2");
    assert_eq!(
        *outcome,
        Outcome::Sent {
            steps: vec![Step::Delete(0)]
        }
    );
    let page_of = Peer::answering(vec![page(
        &[
            (5, release("a/3", &OLD)),
            (4, tombstone("a/1", &OLD)),
            (3, tombstone("a/2", &OLD)),
        ],
        &OLD,
    )]);
    let sweeps = swept(&config, &store, &page_of);
    let [
        Sweep::Polled {
            cursor: remembered, ..
        },
        Sweep::Withdrawn { identity, .. },
        Sweep::Refused(maker),
    ] = sweeps.as_slice()
    else {
        panic!("{sweeps:?}");
    };
    assert_eq!(*remembered, Remembered::Advanced);
    assert_eq!(identity.as_str(), "herald/upstream/a/1");
    assert_eq!(maker, "upstream");
    let kept = cursor(&store).unwrap();
    assert_eq!(kept.sequence, Some(4));
    assert_eq!(kept.held().get("upstream"), Some(&2));
    let shared = |config: &herald_config::Config, store: &Failing| {
        pollster::block_on(herald_flows::shared(config, store))
    };
    assert_eq!(
        shared(&config, &store).notes,
        [
            "sources.up: it keeps 2 tombstones of releases upstream withdrew, all peer_share lets it, so a pull takes no new release of it; raise peer_share to take them."
        ]
    );
    let roomy = deleting(crate::pulls::config("", FROM_PEER));
    assert_eq!(shared(&roomy, &store).notes, Vec::<String>::new());
    assert_eq!(shared(&roomy, &healthy()).notes, Vec::<String>::new());
    let mut unread = store;
    unread.fault = Fault::Cursors;
    assert_eq!(shared(&roomy, &unread).gaps.len(), 1);
}

#[test]
fn tombstones_of_identities_the_source_never_took_never_stop_its_pulls() {
    let config = deleting(config(
        "",
        &format!("{FROM_PEER}\ntombstones = 3\npeer_tombstones = 2"),
    ));
    let store = announcing(&config);
    let flood: Vec<(u64, Value)> = (3..9)
        .rev()
        .map(|sequence| (sequence, tombstone(&format!("x/{sequence}"), &OLD)))
        .collect();
    let sweeps = swept(&config, &store, &Peer::answering(vec![page(&flood, &OLD)]));
    assert_eq!(
        sweeps,
        [Sweep::Polled {
            first: false,
            found: 0,
            announced: Vec::new(),
            cursor: Remembered::Advanced,
        }]
    );
    let kept = cursor(&store).unwrap();
    assert_eq!(kept.sequence, Some(8));
    let unheld: Vec<&str> = kept
        .proved
        .iter()
        .filter(|proof| proof.tombstone.is_some())
        .map(|proof| proof.identity.as_str())
        .collect();
    assert_eq!(unheld, ["herald/upstream/x/8", "herald/upstream/x/7"]);
    let later = Peer::answering(vec![page(
        &[(9, tombstone("a/2", &OLD)), (8, tombstone("x/8", &OLD))],
        &OLD,
    )]);
    let sweeps = swept(&config, &store, &later);
    let (_, identity, outcome) = withdrawn(&sweeps);
    assert_eq!(identity.as_str(), "herald/upstream/a/2");
    assert_eq!(
        *outcome,
        Outcome::Sent {
            steps: vec![Step::Delete(0)]
        }
    );
}

#[test]
fn a_route_keeping_withdrawn_posts_as_it_does_unless_told_keeps_the_one_a_tombstone_withdraws() {
    let config = config("", FROM_PEER);
    assert_eq!(config.routes[0].route.delivery.withdraw, Withdraw::Keep);
    let store = announcing(&config);
    let tombstoned = Peer::answering(vec![page(&[(3, tombstone("a/2", &OLD))], &OLD)]);
    let sweeps = swept(&config, &store, &tombstoned);
    assert_eq!(*withdrawn(&sweeps).2, Outcome::Skipped(KEPT.to_owned()));
    let feed = pollster::block_on(store.inner.feed(&DestinationName::new("mirror")))
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(feed.items[0].envelope["type"], "herald:Event");
}

#[test]
fn a_tombstone_of_what_the_source_never_took_or_the_route_never_announced_withdraws_nothing() {
    let config = deleting(config("", FROM_PEER));
    let store = announcing(&config);
    let unknown = Peer::answering(vec![page(&[(3, tombstone("a/9", &OLD))], &OLD)]);
    assert_eq!(swept(&config, &store, &unknown).len(), 1);
    let quiet = Peer::answering(vec![page(&[(4, tombstone("a/1", &OLD))], &OLD)]);
    let sweeps = swept(&config, &store, &quiet);
    let (_, identity, outcome) = withdrawn(&sweeps);
    assert_eq!(identity.as_str(), "herald/upstream/a/1");
    assert_eq!(*outcome, Outcome::Skipped(UNANNOUNCED.to_owned()));
    let feed = pollster::block_on(store.inner.feed(&DestinationName::new("mirror")))
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(feed.items.len(), 1);
}

#[test]
fn a_tombstone_withdraws_only_the_posts_its_own_source_announced_on_a_shared_route() {
    let granting = format!(
        r#"{FROM_PEER}
namespaces = ["github.com/1"]
[apps.ci]
tokens = ["sha256:0000000000000000000000000000000000000000000000000000000000000000"]
projects = ["php-mutation-gate"]
"#
    );
    let config = deleting(routed("", &granting, "\"up\", \"ci\""));
    let store = healthy();
    let ours = shared(&config, &store, &event("v1"));
    assert_eq!(
        ours,
        Outcome::Sent {
            steps: vec![Step::Post]
        }
    );
    let granted = |version: &str| {
        let mut released = envelope(&format!("github.com/1/{version}"), &[DISCOVERY]);
        released["attributes"]["version"] = json!(version);
        proved(&released, DOMAIN, &OLD)
    };
    pulled(
        &config,
        &store,
        &Peer::answering(vec![page(&[(1, granted("v0"))], &OLD)]),
    );
    let both = Peer::answering(vec![page(
        &[(3, granted("v2")), (2, granted("v1")), (1, granted("v0"))],
        &OLD,
    )]);
    let Sweep::Polled { announced, .. } = pulled(&config, &store, &both) else {
        panic!("a pull");
    };
    assert_eq!(announced.len(), 2);
    let tombstoned = Peer::answering(vec![page(
        &[
            (5, tombstone("github.com/1/v2", &OLD)),
            (4, tombstone("github.com/1/v1", &OLD)),
        ],
        &OLD,
    )]);
    let sweeps = swept(&config, &store, &tombstoned);
    let outcomes: Vec<(&str, &Outcome)> = sweeps
        .iter()
        .filter_map(|sweep| match sweep {
            Sweep::Withdrawn {
                identity,
                announced,
            } => Some((identity.as_str(), &announced[0].outcome)),
            _ => None,
        })
        .collect();
    assert_eq!(
        outcomes,
        [
            (
                "github.com/1/v2",
                &Outcome::Sent {
                    steps: vec![Step::Delete(0)]
                }
            ),
            (
                "github.com/1/v1",
                &Outcome::Skipped(
                    "another source announced the release, so the tombstone leaves its post"
                        .to_owned()
                )
            ),
        ]
    );
}

/// What the route did with `event`, announced by the app `ci` it shares
/// with the peer's source.
fn shared(config: &herald_config::Config, store: &Failing, event: &herald_core::Event) -> Outcome {
    let secrets = FakeSecrets::default().with("HERALD_KEY", "federation-key-of-32-bytes!!!!!!");
    let prepared =
        pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Announces)).unwrap();
    let connected = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let announced = pollster::block_on(announce(
        config,
        &connected,
        &ports,
        &SourceName::new("ci"),
        event,
        "r0",
    ));
    outcome(&announced).clone()
}

#[test]
fn a_tombstone_whose_proof_does_not_hold_withdraws_nothing_and_stops_the_pull() {
    let config = deleting(config("", FROM_PEER));
    let store = announcing(&config);
    let stranger = Peer::answering(vec![page(&[(3, tombstone("a/2", &NEW))], &OLD)]);
    assert_eq!(
        pulled(&config, &store, &stranger),
        Sweep::Stopped(Stop::Source(format!(
            "an envelope is proved by {}, a key not trusted for {DOMAIN}",
            method(DOMAIN, &NEW)
        )))
    );
    let mut forged = tombstone("a/2", &OLD);
    forged["identity"] = json!("a/1");
    let unproved = Peer::answering(vec![page(&[(3, forged)], &OLD)]);
    assert_eq!(
        pulled(&config, &store, &unproved),
        Sweep::Stopped(Stop::Source("an envelope's proof does not hold".to_owned()))
    );
    assert_eq!(cursor(&store).unwrap().sequence, Some(2));
}

#[test]
fn a_withdrawal_that_stops_keeps_the_cursor_and_the_next_round_finishes_it() {
    let config = deleting(config("", FROM_PEER));
    let mut store = announcing(&config);
    store.fault = Fault::WritesFeeds;
    let tombstoned = || Peer::answering(vec![page(&[(3, tombstone("a/2", &OLD))], &OLD)]);
    let sweeps = swept(&config, &store, &tombstoned());
    let (polled, _, outcome) = withdrawn(&sweeps);
    assert!(matches!(outcome, Outcome::Stopped { .. }), "{outcome:?}");
    assert!(matches!(
        polled,
        Sweep::Polled {
            cursor: Remembered::Kept,
            ..
        }
    ));
    assert_eq!(cursor(&store).unwrap().sequence, Some(2));
    store.fault = Fault::Nothing;
    let rounded: Vec<(String, Sweep)> = rounded(&config, &store, &tombstoned())
        .into_iter()
        .map(|swept| (swept.source.as_str().to_owned(), swept.outcome))
        .collect();
    let gone = |outcome: Outcome| {
        vec![Announced {
            route: RouteId::new("mirror"),
            outcome,
        }]
    };
    let identity = Identity::new("herald/upstream/a/2");
    assert_eq!(
        rounded,
        [
            (
                "mirror".to_owned(),
                Sweep::Withdrawn {
                    identity: identity.clone(),
                    announced: gone(Outcome::Sent {
                        steps: vec![Step::Delete(0)]
                    }),
                }
            ),
            (
                "up".to_owned(),
                Sweep::Polled {
                    first: false,
                    found: 0,
                    announced: Vec::new(),
                    cursor: Remembered::Advanced,
                }
            ),
            (
                "up".to_owned(),
                Sweep::Withdrawn {
                    identity,
                    announced: gone(Outcome::Skipped(UNANNOUNCED.to_owned())),
                }
            ),
        ]
    );
    assert_eq!(cursor(&store).unwrap().sequence, Some(3));
}

#[test]
fn a_route_announcing_each_project_as_its_own_actor_keeps_what_a_tombstone_withdraws() {
    let acting = format!(
        r#"{FROM_PEER}
[destinations.fedi]
kind = "activitypub"
domain = "herald.self"
key = {{ secret = "HERALD_KEY" }}
[[routes]]
id = "actors"
sources = ["up"]
destination = "fedi"
template = "t"
actor = "{{{{ event.project }}}}"
withdraw = "delete"
private = false
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
"#
    );
    let config = config("", &acting);
    let store = healthy();
    let quiet = Peer::answering(vec![page(&[(1, release("a/2", &OLD))], &OLD)]);
    pulled(&config, &store, &quiet);
    let tombstoned = Peer::answering(vec![page(&[(2, tombstone("a/2", &OLD))], &OLD)]);
    let sweeps = swept(&config, &store, &tombstoned);
    let [_, Sweep::Withdrawn { announced, .. }] = sweeps.as_slice() else {
        panic!("{sweeps:?}");
    };
    assert_eq!(
        announced[0],
        Announced {
            route: RouteId::new("actors"),
            outcome: Outcome::Skipped(
                "the route announces each project as an actor of its own, and a tombstone names no project, so its post stays".to_owned()
            ),
        }
    );
}

#[test]
fn a_tombstone_whose_post_cannot_be_read_stops_the_withdrawal_holding_nothing() {
    let config = deleting(config("", FROM_PEER));
    let mut store = announcing(&config);
    store.fault = Fault::ReadsOnce;
    *store.landings.lock().unwrap() = 0;
    let tombstoned = Peer::answering(vec![page(&[(3, tombstone("a/2", &OLD))], &OLD)]);
    let sweeps = swept(&config, &store, &tombstoned);
    let (polled, _, outcome) = withdrawn(&sweeps);
    assert!(matches!(outcome, Outcome::Stopped { .. }), "{outcome:?}");
    assert!(matches!(
        polled,
        Sweep::Polled {
            cursor: Remembered::Kept,
            ..
        }
    ));
    store.fault = Fault::Nothing;
    let quiet = Peer::answering(vec![page(&[(2, release("a/2", &OLD))], &OLD)]);
    let sources: Vec<String> = rounded(&config, &store, &quiet)
        .into_iter()
        .map(|swept| swept.source.as_str().to_owned())
        .collect();
    assert_eq!(sources, ["up"]);
}
