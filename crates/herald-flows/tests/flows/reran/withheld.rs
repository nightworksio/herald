//! A private or internal repository's plans withheld and counted unless
//! the output is said private, and a job of a withheld plan refused naming
//! nothing (ADR-0069).

use herald_config::Uses;
use herald_core::Printed;
use herald_flows::{
    Budget, Connected, Ports, RerunPlans, UNPLANNED_WITHHELD, prepare, rerun_plans,
};
use herald_testing::{FakeClock, FakeSigner};

use super::{CHOSEN, TOKEN_WRITE, applied_from, config, planned, refused, scripted, secrets};
use crate::harness::{NOON, Script, healthy};

#[test]
fn a_private_repository_s_plan_is_withheld_and_counted_unless_the_output_is_said_private() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    crate::boards::flaky_seen(&store, "Change", herald_core::Visibility::Private);
    let script = scripted(&[planned(), planned()]);
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let flow = (&config, &connected, &ports);
    let withheld = pollster::block_on(rerun_plans(flow, (None, Printed::Withheld))).unwrap();
    assert_eq!(
        withheld,
        RerunPlans {
            planned: Vec::new(),
            withheld: 1
        }
    );
    assert!(script.seen().is_empty(), "{:?}", script.seen());
    let board = pollster::block_on(herald_flows::queue(
        &config,
        &connected,
        &ports,
        ("kept", Printed::Withheld),
    ))
    .unwrap();
    assert_eq!((board.total(), board.withheld), (0, 1));
    assert!(board.groups.iter().all(|group| group.rows.is_empty()));
    for printed in [Printed::Configured, Printed::Asked] {
        let plans = pollster::block_on(rerun_plans(flow, (None, printed))).unwrap();
        assert_eq!((plans.planned.len(), plans.withheld), (1, 0), "{printed:?}");
        let board = pollster::block_on(herald_flows::queue(
            &config,
            &connected,
            &ports,
            ("kept", printed),
        ))
        .unwrap();
        assert_eq!((board.total(), board.withheld), (1, 0), "{printed:?}");
    }
}

#[test]
fn a_job_of_a_withheld_plan_is_refused_saying_the_output_must_be_said_private_and_naming_nothing() {
    let config = config(TOKEN_WRITE);
    let store = healthy();
    crate::boards::flaky_seen(&store, "Change", herald_core::Visibility::Internal);
    let script = scripted(&[]);
    let applied = applied_from(
        &config,
        (&config, &store, &secrets()),
        &script,
        (CHOSEN, None),
        (None, Printed::Withheld),
    );
    assert_eq!(refused(&applied), UNPLANNED_WITHHELD);
    let only = &applied.applied.as_ref().unwrap()[0];
    assert_eq!((&only.repository, only.number), (&None, None));
    assert!(!UNPLANNED_WITHHELD.contains("o/open"));
    assert!(applied.told.is_empty() && script.seen().is_empty());
}
