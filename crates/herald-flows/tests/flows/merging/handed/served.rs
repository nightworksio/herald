//! `herald server` steps the merge queues whose `applied_by = "server"`
//! as `herald merge-queue --apply` steps the others: the same fresh read
//! right before the call, the same record first, the same write
//! credential held apart, and the same lost answer read back.

use std::sync::Mutex;

use herald_config::Uses;
use herald_core::SourceName;
use herald_core::merging::HandedOff;
use herald_flows::{
    Budget, Connected, Found, GONE, Handing, MergeQueueRead, Next, Ports, Took, UNSEPARATED,
    merge_served, prepare, served,
};
use herald_ports::Base;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::json;

use super::{
    EVENTS, Forge, GITHUB_API, GRAPHQL, PERMISSION, PULL, QUEUE, QUEUED, answer, labeled, pull,
    queued, record, scripted, the_secrets, writer,
};
use crate::harness::{Failing, NOON, config, healthy, secrets};

const SERVED_BY: &str = "applied_by = \"server\"\n";

/// What `herald server` reads and steps of the queues `queued` configures,
/// woken by `source` where it is some, through `forge`, with `held`
/// secrets, its records kept in `store`; with each base a write connection
/// was made for.
fn serve(
    queued: &str,
    (store, forge, held): (&Failing, &Forge, &FakeSecrets),
    source: Option<&str>,
) -> (Vec<MergeQueueRead>, Vec<String>) {
    let config = config(queued);
    let prepared = pollster::block_on(prepare(&config, held, &FakeSigner, Uses::Serves)).unwrap();
    let connected: Connected<Forge> = prepared.connect(|_| Ok::<_, ()>(forge.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let bases = Mutex::new(Vec::new());
    let connect = |base: Base<'_>| {
        if let Base::At(at) = base {
            bases.lock().unwrap().push(at.to_owned());
        }
        Ok(forge.clone())
    };
    let handing = Handing {
        secrets: held,
        connect: &connect,
        timer: forge,
    };
    assert_eq!(format!("{handing:?}"), "Handing { .. }");
    let source = source.map(SourceName::new);
    let read = pollster::block_on(merge_served(
        (&config, &connected, &ports),
        &handing,
        source.as_ref(),
    ));
    (read, bases.into_inner().unwrap())
}

/// What `herald server` comes to with the one queue [`QUEUE`] and `extra`
/// configure, its steps the server's.
fn nexts(extra: &str, (store, forge, held): (&Failing, &Forge, &FakeSecrets)) -> Vec<Next> {
    let (read, bases) = serve(
        &format!("{QUEUE}{SERVED_BY}{extra}"),
        (store, forge, held),
        None,
    );
    assert!(bases.iter().all(|base| base == GITHUB_API), "{bases:?}");
    let [read] = &read[..] else {
        panic!("{read:?}");
    };
    assert_eq!(read.idle, Vec::<String>::new());
    let Found::Entries(entries) = &read.found else {
        panic!("{read:?}");
    };
    entries.iter().map(|judged| judged.next.clone()).collect()
}

#[test]
fn the_server_hands_a_pull_request_over_once_with_the_write_credential_alone() {
    let store = healthy();
    let forge = scripted();
    assert_eq!(
        nexts("", (&store, &forge, &the_secrets())),
        [Next::Took(Took::Queued)]
    );
    assert_eq!(forge.carrying("write-value"), [GRAPHQL]);
    let lines = forge.lines();
    let call = lines.iter().position(|line| line == GRAPHQL).unwrap();
    let fresh = lines[..call].iter().rposition(|line| line == PULL).unwrap();
    for read in [EVENTS, PERMISSION, QUEUED] {
        assert!(lines[fresh..call].contains(&read.to_owned()), "{lines:?}");
    }
    let handed = record(&store).unwrap().hand_off.unwrap();
    assert_eq!(
        (
            handed.head.as_str(),
            handed.credential.as_str(),
            handed.outcome
        ),
        ("h7", "HERALD_MERGE", HandedOff::Queued)
    );
    let again = scripted();
    assert_eq!(
        nexts("", (&store, &again, &the_secrets())),
        [Next::Handed {
            outcome: HandedOff::Queued,
            at: NOON
        }]
    );
    assert_eq!(again.carrying("write-value"), Vec::<String>::new());
}

#[test]
fn the_server_reads_again_right_before_the_step_and_hands_nothing_over_that_moved() {
    let moved = scripted().answering(PULL, &[answer(200, &pull("h8", &["herald:merge"]))]);
    let unroled = scripted().answering(PERMISSION, &[writer(true), writer(false)]);
    let cancelled = scripted().answering(
        EVENTS,
        &[
            answer(200, &json!([labeled()])),
            answer(
                200,
                &json!([labeled(), { "event": "unlabeled", "actor": { "login": "x", "id": 3 },
                "created_at": "2026-11-01T11:30:00Z", "label": { "name": "herald:merge" } }]),
            ),
        ],
    );
    for (forge, took) in [
        (moved, Took::HeadMoved),
        (unroled, Took::Refused(GONE.to_owned())),
        (cancelled, Took::Refused(GONE.to_owned())),
    ] {
        let store = healthy();
        assert_eq!(
            nexts("", (&store, &forge, &the_secrets())),
            [Next::Took(took)]
        );
        assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
        assert_eq!(forge.carrying("write-value"), Vec::<String>::new());
        assert_eq!(record(&store), None);
    }
}

#[test]
fn the_server_holds_the_write_credential_apart_and_reads_a_lost_answer_back() {
    let store = healthy();
    let forge = scripted();
    let same = secrets()
        .with("HERALD_READ", "one-value")
        .with("HERALD_MERGE", "one-value");
    assert_eq!(
        nexts("", (&store, &forge, &same)),
        [Next::Took(Took::Refused(UNSEPARATED.to_owned()))]
    );
    assert!(!forge.lines().contains(&GRAPHQL.to_owned()));
    let lost = scripted()
        .answering(GRAPHQL, &[answer(502, &json!({}))])
        .answering(QUEUED, &[queued(false), queued(false)]);
    let store = healthy();
    assert_eq!(
        nexts("", (&store, &lost, &the_secrets())),
        [Next::Took(Took::Unsettled)]
    );
    assert_eq!(lost.carrying("write-value"), [GRAPHQL]);
    let again = scripted();
    assert_eq!(
        nexts("", (&store, &again, &the_secrets())),
        [Next::Handed {
            outcome: HandedOff::Unsettled,
            at: NOON
        }]
    );
    assert!(!again.lines().contains(&GRAPHQL.to_owned()));
}

#[test]
fn the_server_steps_only_its_own_queues_and_those_of_the_source_that_woke_it() {
    let store = healthy();
    let forge = scripted();
    let held = the_secrets();
    assert_eq!(
        serve(QUEUE, (&store, &forge, &held), None),
        (Vec::new(), Vec::new())
    );
    let queued = format!("{QUEUE}{SERVED_BY}");
    assert_eq!(
        serve(&queued, (&store, &forge, &held), Some("other")),
        (Vec::new(), Vec::new())
    );
    assert_eq!(forge.lines(), Vec::<String>::new());
    assert_eq!(served(&config(QUEUE), None), Vec::<&str>::new());
    assert_eq!(served(&config(&queued), None), ["app"]);
    let (read, _) = serve(&queued, (&store, &forge, &held), Some("app"));
    assert_eq!(read.len(), 1);
    assert!(forge.lines().contains(&GRAPHQL.to_owned()));
}

#[test]
fn the_server_steps_a_private_repository_s_queue_and_says_only_how_many() {
    let private = scripted().answering(
        "GET https://api.github.com/repos/o/r",
        &[answer(200, &json!({ "private": true }))],
    );
    let store = healthy();
    let (read, _) = serve(
        &format!("{QUEUE}{SERVED_BY}"),
        (&store, &private, &the_secrets()),
        None,
    );
    assert_eq!(read[0].found, Found::Withheld(1));
    assert_eq!(private.carrying("write-value"), [GRAPHQL]);
    assert_eq!(
        record(&store).unwrap().hand_off.unwrap().outcome,
        HandedOff::Queued
    );
}
