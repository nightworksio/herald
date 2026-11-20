use herald_config::{Config, Guard};
use herald_core::{Draft, Event, Kind, SourceName};
use herald_flows::{Outcome, Ports, announce, prepare};
use herald_testing::{FakeClock, FakeSigner};

use crate::harness::*;

const UNPLACED: &str = "the event's kind belongs in private places, and the route says nothing of whether its place is private";

fn of(kind: Kind) -> Event {
    Event::normalise(Draft {
        identity: "github.com/1/alert/7".to_owned(),
        kind,
        project: "php-mutation-gate".to_owned(),
        attributes: herald_core::versioned("1.0.0".to_owned(), false),
        title: "php-mutation-gate".to_owned(),
        body: "## Found".to_owned(),
        links: [("release".to_owned(), "https://example.org/r".to_owned())].into(),
        occurred: NOON,
    })
    .unwrap()
}

fn announced(config: &Config, event: &Event) -> Outcome {
    let store = healthy();
    let crossposted = config.routes[0].route.delivery.crosspost;
    let answers: Vec<_> = [POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]
        .into_iter()
        .filter(|answer| crossposted || *answer != CROSSPOSTED)
        .collect();
    let script = Script::answering(&answers);
    let prepared = pollster::block_on(prepare(
        config,
        &secrets(),
        &FakeSigner,
        herald_config::Uses::Polls,
    ))
    .unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store: &store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let announced = pollster::block_on(announce(
        config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        event,
        "r1",
    ));
    outcome(&announced).clone()
}

#[test]
fn a_private_kind_reaches_no_route_that_says_nothing_of_its_place() {
    let skipped = Outcome::Skipped(UNPLACED.to_owned());
    let unstated = config("");
    assert!(!unstated.routes[0].stated);
    assert_eq!(announced(&unstated, &of(Kind::ALERT)), skipped);
    let unknown = of(Kind::new("deploy").unwrap());
    assert_eq!(announced(&unstated, &unknown), skipped);
    assert!(matches!(
        announced(&unstated, &of(Kind::RELEASE)),
        Outcome::Sent { .. }
    ));
    for stated in ["private = true", "private = false"] {
        let config = match stated {
            "private = true" => crate::harness::private_config(""),
            _ => config(stated),
        };
        assert!(config.routes[0].stated, "{stated}");
        assert!(
            matches!(announced(&config, &of(Kind::ALERT)), Outcome::Sent { .. }),
            "{stated}"
        );
    }
    for (guard, sent) in [
        (Guard::Warn, true),
        (Guard::Allow, true),
        (Guard::Refuse, false),
    ] {
        let mut config = unstated.clone();
        config.doctor.private_kinds = guard;
        assert_eq!(
            matches!(announced(&config, &of(Kind::ALERT)), Outcome::Sent { .. }),
            sent,
            "{guard:?}"
        );
    }
}
