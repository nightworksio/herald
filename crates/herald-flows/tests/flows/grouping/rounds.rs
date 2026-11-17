use herald_core::{HeldEvent, Pending, Step};
use herald_flows::{Outcome, Stop, Sweep};
use herald_ports::Waiting;

use super::*;

const UNREAD: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages?limit=100",
    200,
    "[]",
);

fn kept(store: &Failing) -> Option<Pending> {
    pollster::block_on(store.inner.pending(&hold()))
        .unwrap()
        .map(|kept| kept.value)
}

#[test]
fn a_group_whose_route_no_longer_groups_or_holds_no_member_is_forgotten() {
    let (config, store) = (grouped("", ""), healthy());
    let script = Script::default();
    sent(&config, (&store, NOON), &script, &release("cli", 2));
    let ungrouped = crate::harness::config("");
    assert_eq!(
        swept(&ungrouped, (&store, at(30)), &script),
        Vec::<Swept>::new()
    );
    assert_eq!(kept(&store), None);
    let empty = Pending {
        hold: hold(),
        events: Vec::<HeldEvent>::new(),
        opened: NOON,
        due: NOON,
        closed: None,
        landed: false,
    };
    pollster::block_on(store.inner.put_pending(&empty, None)).unwrap();
    assert_eq!(
        swept(&grouped("", ""), (&store, at(30)), &script),
        Vec::<Swept>::new()
    );
    assert_eq!(kept(&store), None);
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn a_group_that_cannot_be_forgotten_is_said_under_its_route() {
    let (config, store) = (
        grouped("", ""),
        Failing {
            fault: Fault::DropsPending,
            ..healthy()
        },
    );
    sent(
        &config,
        (&store, NOON),
        &Script::default(),
        &release("cli", 2),
    );
    let swept = swept(
        &crate::harness::config(""),
        (&store, at(30)),
        &Script::default(),
    );
    assert_eq!(swept.len(), 1);
    assert_eq!(swept[0].source, SourceName::new("train"));
    assert_eq!(swept[0].member, None);
    assert_eq!(
        swept[0].outcome,
        Sweep::Stopped(Stop::Store(
            "the store could not be reached: broken".to_owned()
        ))
    );
}

#[test]
fn an_announced_group_is_kept_as_long_as_its_ledger_entry_then_forgotten() {
    let (config, store) = (grouped("", ""), healthy());
    let script = Script::answering(&[POSTED]);
    sent(&config, (&store, NOON), &script, &release("cli", 2));
    swept(&config, (&store, at(30)), &script);
    let ninety_days = 90 * 24 * 60;
    assert_eq!(
        swept(&config, (&store, at(30 + ninety_days - 1)), &script),
        Vec::<Swept>::new()
    );
    assert!(kept(&store).is_some());
    assert_eq!(
        swept(&config, (&store, at(30 + ninety_days)), &script),
        Vec::<Swept>::new()
    );
    assert_eq!(kept(&store), None);
}

#[test]
fn a_group_whose_post_stopped_is_sent_again_by_the_next_round() {
    let (config, store) = (grouped("", ""), healthy());
    sent(
        &config,
        (&store, NOON),
        &Script::default(),
        &release("cli", 2),
    );
    let unreachable = Script::default();
    let Outcome::Stopped { reason, .. } =
        round_outcomes(&swept(&config, (&store, at(30)), &unreachable)).remove(0)
    else {
        panic!("an unreachable destination stops the group");
    };
    assert!(matches!(reason, Stop::Unreachable(_)), "{reason:?}");
    let closed = kept(&store).unwrap();
    assert_eq!((closed.closed, closed.landed), (Some(at(30)), false));
    let reachable = Script::answering(&[WHO, UNREAD, POSTED]);
    let finished = round_outcomes(&swept(&config, (&store, at(35)), &reachable));
    assert!(
        matches!(&finished[0], Outcome::Sent { steps } if steps.contains(&Step::Post)),
        "{finished:?}"
    );
    assert!(kept(&store).unwrap().landed);
}

#[test]
fn a_group_that_edits_whose_first_post_stopped_is_posted_once_its_window_passes() {
    let (config, store) = (grouped(", mode = \"edit\"", ""), healthy());
    let Outcome::Stopped { .. } = sent(
        &config,
        (&store, NOON),
        &Script::default(),
        &release("cli", 2),
    ) else {
        panic!("an unreachable destination stops the group");
    };
    assert_eq!(
        swept(&config, (&store, at(29)), &Script::default()),
        Vec::<Swept>::new()
    );
    let reachable = Script::answering(&[WHO, UNREAD, POSTED]);
    let finished = round_outcomes(&swept(&config, (&store, at(30)), &reachable));
    assert!(matches!(&finished[0], Outcome::Sent { .. }), "{finished:?}");
    let kept = kept(&store).unwrap();
    assert_eq!((kept.closed, kept.landed), (None, true));
}

#[test]
fn a_member_announced_in_a_group_is_never_announced_alone_once_its_route_stops_grouping() {
    let (config, store) = (grouped(", until = \"lead\"", ""), healthy());
    let script = Script::answering(&[POSTED]);
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("core", 1)),
        posted()
    );
    let mut ungrouped = config.clone();
    ungrouped.routes[0].route.delivery.group = None;
    assert_eq!(
        sent(&ungrouped, (&store, at(5)), &script, &release("core", 1)),
        Outcome::AlreadyAnnounced
    );
    assert_eq!(script.seen().len(), 1);
}
