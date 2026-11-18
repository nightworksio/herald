//! A release's posts withdrawn as the operator asks, whatever each route's
//! `withdraw` says, and what a withdrawal would remove (ADR-0048).

use herald_config::{Config, Uses, load};
use herald_core::{Identity, RouteId, Step};
use herald_flows::{
    Announced, Budget, Outcome, Ports, Withdrawable, WithdrawablePost, prepare, withdraw,
    withdrawable,
};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};

use super::{DELETED, entry, kept, message};
use crate::harness::{Failing, Fault, NOON, Script, healthy};

/// Why a route announcing each project as an actor of its own withdraws
/// nothing the operator asks it to.
const ACTING: &str = "the route announces each project as an actor of its own, and the operator names no project, so only the forge's deletion of the release withdraws it";
/// Why a route withdraws nothing: its destination deletes no posts.
const UNDELETABLE: &str = "the destination deletes no posts, so the post stays";

fn config() -> Config {
    let text = r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[destinations.hook]
kind = "slack"
webhook = { secret = "HERALD_SLACK_WEBHOOK" }
[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
[[routes]]
id = "hooked"
sources = ["gate"]
destination = "hook"
template = "t"
[[routes]]
id = "actors"
sources = ["gate"]
destination = "fedi"
template = "t"
actor = "{{ event.project | replace('.', '_') }}"
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#;
    load(
        "herald.toml",
        text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn secrets() -> FakeSecrets {
    FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "t")
        .with(
            "HERALD_SLACK_WEBHOOK",
            "https://hooks.slack.com/services/T/B/x",
        )
        .with("HERALD_ACTIVITYPUB_KEY", "k")
}

/// What withdrawing `identity` from `route`, or every route, did, or would.
fn asked(
    store: &Failing,
    script: &Script,
    route: Option<&str>,
    dry: bool,
) -> Result<Vec<Announced>, Vec<Withdrawable>> {
    let config = config();
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Announces)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let route = route.map(RouteId::new);
    let target = (route.as_ref(), &Identity::new("github.com/1/v1"));
    if dry {
        Err(pollster::block_on(withdrawable(&config, &connected, &ports, target)).unwrap())
    } else {
        Ok(pollster::block_on(withdraw(
            &config, &connected, &ports, target, "r1",
        )))
    }
}

#[test]
fn the_operator_withdraws_from_a_route_that_keeps_and_every_route_says_what_it_did() {
    let store = healthy();
    kept(&store, "github.com/1/v1", |entry| {
        entry.landed(Step::Post, message("1"))
    });
    let shown = herald_core::Latest::after(
        None,
        &crate::harness::event("v1"),
        &RouteId::new("releases"),
    );
    pollster::block_on(herald_ports::Announcements::put_latest(
        &store.inner,
        &shown,
        None,
    ))
    .unwrap();
    let script = Script::answering(&[DELETED]);
    let Ok(one) = asked(&store, &script, Some("releases"), false) else {
        unreachable!()
    };
    assert_eq!(
        pollster::block_on(herald_ports::Announcements::latest(
            &store.inner,
            "php-mutation-gate"
        ))
        .unwrap(),
        None
    );
    assert_eq!(
        one,
        [Announced {
            route: RouteId::new("releases"),
            outcome: Outcome::Sent {
                steps: vec![Step::Delete(0)]
            },
        }]
    );
    assert_eq!(
        entry(&store, "github.com/1/v1").unwrap().withdrawn,
        Some(NOON)
    );
    let Ok(every) = asked(&store, &script, None, false) else {
        unreachable!()
    };
    let outcomes: Vec<(&str, &Outcome)> = every
        .iter()
        .map(|done| (done.route.as_str(), &done.outcome))
        .collect();
    assert_eq!(
        outcomes,
        [
            ("releases", &Outcome::Skipped(super::UNANNOUNCED.to_owned())),
            ("hooked", &Outcome::Skipped(UNDELETABLE.to_owned())),
            ("actors", &Outcome::Skipped(ACTING.to_owned())),
        ]
    );
    let Ok(none) = asked(&store, &script, Some("gone"), false) else {
        unreachable!()
    };
    assert_eq!(none, []);
}

#[test]
fn a_dry_run_names_each_post_it_would_remove_and_why_a_route_keeps_it() {
    let store = healthy();
    kept(&store, "github.com/1/v1", |entry| {
        entry
            .landed(Step::Post, message("1"))
            .landed(Step::Chunk(1), message("2"))
    });
    kept(&store, "github.com/1/v1#release", |mut entry| {
        entry.group = Some("v1".to_owned());
        entry
    });
    let script = Script::default();
    let Err(planned) = asked(&store, &script, None, true) else {
        unreachable!()
    };
    assert_eq!(
        planned,
        [
            Withdrawable {
                route: RouteId::new("releases"),
                posts: vec![
                    WithdrawablePost {
                        identity: Identity::new("github.com/1/v1"),
                        steps: vec![Step::Delete(1), Step::Delete(0)],
                        group: None,
                    },
                    WithdrawablePost {
                        identity: Identity::new("github.com/1/v1#release"),
                        steps: Vec::new(),
                        group: Some("v1".to_owned()),
                    },
                ],
                kept: None,
            },
            Withdrawable {
                route: RouteId::new("hooked"),
                posts: Vec::new(),
                kept: Some(UNDELETABLE.to_owned()),
            },
            Withdrawable {
                route: RouteId::new("actors"),
                posts: Vec::new(),
                kept: Some(ACTING.to_owned()),
            },
        ]
    );
    assert_eq!(script.seen(), Vec::<String>::new());
    assert!(
        entry(&store, "github.com/1/v1")
            .unwrap()
            .withdrawn
            .is_none()
    );
    kept(&store, "github.com/1/v1#release", |mut entry| {
        entry.withdrawn = Some(NOON);
        entry
    });
    let Err(again) = asked(&store, &script, Some("releases"), true) else {
        unreachable!()
    };
    assert_eq!(again[0].posts.len(), 1);
}

#[test]
fn a_dry_run_over_a_ledger_it_cannot_read_stops() {
    let mut store = healthy();
    store.fault = Fault::Reads;
    let config = config();
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Announces)).unwrap();
    let connected = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    let ports = Ports {
        store: &store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let target = (
        Some(&RouteId::new("releases")),
        &Identity::new("github.com/1/v1"),
    );
    assert!(pollster::block_on(withdrawable(&config, &connected, &ports, target)).is_err());
}

/// What a round at noon did with what the store holds, under `config`.
fn round(config: &Config, store: &Failing) -> Vec<herald_flows::Swept> {
    let prepared =
        pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Announces)).unwrap();
    let connected = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    pollster::block_on(herald_flows::announce_held(
        config, &connected, &ports, "r2",
    ))
    .unwrap()
}

/// A withdrawal record of `route`, holding the forge's deletion of a
/// release of `project` where it names one.
fn recorded(store: &Failing, route: &str, project: Option<&str>) -> herald_core::Hold {
    let hold = herald_core::Hold::Withdraw {
        route: RouteId::new(route),
        identity: Identity::new("github.com/1/v1"),
    };
    let events = project
        .map(|project| {
            let mut written = serde_json::to_value(crate::harness::event("v1")).unwrap();
            written["project"] = project.into();
            herald_core::HeldEvent {
                source: herald_core::SourceName::new("gate"),
                member: None,
                event: serde_json::from_value::<herald_core::Event>(written)
                    .unwrap()
                    .marked_withdrawn(),
            }
        })
        .into_iter()
        .collect();
    let pending = herald_core::Pending {
        hold: hold.clone(),
        events,
        opened: NOON,
        due: NOON,
        closed: None,
        landed: false,
    };
    pollster::block_on(herald_ports::Waiting::put_pending(
        &store.inner,
        &pending,
        None,
    ))
    .unwrap();
    hold
}

fn outcome_of(swept: &[herald_flows::Swept]) -> Outcome {
    let [
        herald_flows::Swept {
            outcome: herald_flows::Sweep::Withdrawn { announced, .. },
            ..
        },
    ] = swept
    else {
        panic!("{swept:?}");
    };
    announced[0].outcome.clone()
}

fn kept_record(store: &Failing, hold: &herald_core::Hold) -> bool {
    pollster::block_on(herald_ports::Waiting::pending(&store.inner, hold))
        .unwrap()
        .is_some()
}

#[test]
fn a_round_finishes_each_route_s_withdrawal_and_lets_go_of_one_it_cannot_finish() {
    let store = healthy();
    let gone = recorded(&store, "gone", None);
    let hooked = recorded(&store, "hooked", None);
    let acting = recorded(&store, "actors", Some("php-mutation-gate"));
    let swept = round(&config(), &store);
    assert_eq!(swept.len(), 2, "{swept:?}");
    let outcomes: Vec<Outcome> = swept
        .iter()
        .map(|one| outcome_of(std::slice::from_ref(one)))
        .collect();
    assert!(
        outcomes.contains(&Outcome::Skipped(UNDELETABLE.to_owned())),
        "{outcomes:?}"
    );
    assert!(
        outcomes.contains(&Outcome::Skipped(super::UNANNOUNCED.to_owned())),
        "{outcomes:?}"
    );
    for hold in [gone, hooked, acting] {
        assert!(!kept_record(&store, &hold), "{hold:?}");
    }
}

#[test]
fn a_round_whose_release_names_no_actor_lets_it_go_and_one_that_cannot_let_go_says_so() {
    let mut store = healthy();
    let unnamed = recorded(&store, "actors", Some("Night Works"));
    let outcome = outcome_of(&round(&config(), &store));
    assert!(matches!(outcome, Outcome::Skipped(_)), "{outcome:?}");
    assert!(!kept_record(&store, &unnamed));
    store.fault = Fault::DropsPending;
    for route in ["gone", "hooked"] {
        let hold = recorded(&store, route, None);
        let swept = round(&config(), &store);
        assert!(
            matches!(
                swept[..],
                [herald_flows::Swept {
                    outcome: herald_flows::Sweep::Stopped(_),
                    ..
                }]
            ),
            "{route} {swept:?}"
        );
        pollster::block_on(herald_ports::Waiting::drop_pending(&store.inner, &hold)).unwrap();
    }
}
