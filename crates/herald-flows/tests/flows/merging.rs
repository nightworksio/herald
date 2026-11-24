//! `herald merge-queue`'s dry run reads each queue named, or every one,
//! through the source it names alone.

use std::collections::BTreeMap;

use herald_config::Uses;
use herald_core::Printed;
use herald_flows::{Budget, Connected, Found, Ports, merge_branch, merge_queues, prepare};
use herald_testing::{FakeClock, FakeSigner};

use crate::harness::{NOON, Script, config, healthy, secrets};

mod handed;

const QUEUES: &str = "[sources.other]\nkind = \"github\"\nrepository = \"acme/other\"\ntoken = { secret = \"HERALD_OTHER\" }\n[merge_queues.a]\nsource = \"gate\"\nbranch = \"main\"\n[merge_queues.b]\nsource = \"other\"\nbranch = \"main\"\n[merge_queues.c]\nsource = \"gate\"\nbranch = \"dev\"\n";

/// Each queue read, as its name, repository and whether it is unread with
/// a reason; each request's line; and whether each carried the other
/// source's credential.
type Read = (Vec<(String, String, bool)>, Vec<String>, Vec<bool>);

fn read(name: Option<&str>) -> Read {
    let config = config(QUEUES);
    let prepared = pollster::block_on(prepare(
        &config,
        &secrets().with("HERALD_OTHER", "ghp_other"),
        &FakeSigner,
        Uses::Polls,
    ))
    .unwrap();
    let script = Script::keyed(BTreeMap::new());
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let store = healthy();
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let read = pollster::block_on(merge_queues(
        (&config, &connected, &ports),
        (name, Printed::Asked),
    ))
    .unwrap();
    (
        read.into_iter()
            .map(|queue| {
                let unread = matches!(queue.found, Found::Unread(Some(_)));
                (queue.name, queue.repository, unread)
            })
            .collect(),
        script.seen(),
        script
            .asked()
            .iter()
            .map(|headers| headers.iter().any(|(_, value)| value.contains("ghp_other")))
            .collect(),
    )
}

#[test]
fn every_queue_is_read_once_in_name_order_through_its_own_source() {
    let (read, _, _) = read(None);
    let gate = "nightworksio/php-mutation-gate".to_owned();
    assert_eq!(
        read,
        [
            ("a".to_owned(), gate.clone(), true),
            ("b".to_owned(), "acme/other".to_owned(), true),
            ("c".to_owned(), gate, true),
        ]
    );
}

#[test]
fn a_queue_named_is_read_alone_on_its_own_repository_with_its_own_source_s_credential() {
    let (read, seen, carried) = read(Some("b"));
    assert_eq!(read, [("b".to_owned(), "acme/other".to_owned(), true)]);
    assert_ne!(seen, Vec::<String>::new());
    assert!(
        seen.iter().all(|line| line.contains("/repos/acme/other")),
        "{seen:?}"
    );
    assert_eq!(carried, vec![true; seen.len()]);
}

#[test]
fn a_queue_s_base_branch_is_read_through_its_own_source_and_a_failure_said_where_printed() {
    let config = config(QUEUES);
    let prepared = pollster::block_on(prepare(
        &config,
        &secrets().with("HERALD_OTHER", "ghp_other"),
        &FakeSigner,
        Uses::Polls,
    ))
    .unwrap();
    let script = Script::keyed(BTreeMap::new());
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let store = healthy();
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let flow = (&config, &connected, &ports);
    let said = pollster::block_on(merge_branch(flow, ("c", Printed::Asked)));
    assert!(matches!(said, Err(Some(_))), "{said:?}");
    assert!(
        script
            .seen()
            .iter()
            .all(|line| line.contains("php-mutation-gate")),
        "{:?}",
        script.seen()
    );
    let withheld = pollster::block_on(merge_branch(flow, ("c", Printed::of(false, false))));
    assert_eq!(withheld, Err(None));
    let unknown = pollster::block_on(merge_branch(flow, ("d", Printed::Asked)));
    assert!(
        matches!(unknown, Err(Some(ref why)) if why.contains("\"d\"")),
        "{unknown:?}"
    );
}

#[test]
fn a_queue_whose_source_s_installation_token_is_not_minted_has_its_branch_unread() {
    let config = config(
        "[sources.minted]\nkind = \"github\"\nrepository = \"acme/minted\"\napp = \"herald\"\n[github_apps.herald]\nclient_id = \"Iv23liABCDEF\"\nprivate_key = { secret = \"HERALD_APP_KEY\" }\n[merge_queues.m]\nsource = \"minted\"\nbranch = \"main\"\n",
    );
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let script = Script::keyed(BTreeMap::new());
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let store = healthy();
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let said = pollster::block_on(merge_branch(
        (&config, &connected, &ports),
        ("m", Printed::Asked),
    ));
    assert!(
        matches!(said, Err(Some(ref why)) if why.starts_with("its source's installation token could not be minted")),
        "{said:?}"
    );
    assert!(
        script
            .seen()
            .iter()
            .all(|line| !line.contains("/branches/")),
        "{:?}",
        script.seen()
    );
}
