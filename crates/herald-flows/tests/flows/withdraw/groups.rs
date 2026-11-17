//! A release the forge deleted taken out of the group it was gathered into:
//! the group's post brought to the members left, or withdrawn where none
//! is left (ADR-0048).

use herald_core::{Identity, Key, RouteId, Signal, Step, group_identity};
use herald_flows::Outcome;
use herald_ports::{Ledger, Waiting};

use crate::grouping::{
    EDITED, at, edited, gathered, grouped, hold, member, posted, release, round_outcomes, sent,
    swept, unrevised,
};
use crate::harness::{Failing, POSTED, Script, healthy};

fn deleted(project: &str, id: u32) -> herald_core::Event {
    member(project, id, Signal::Release, "notes").marked_withdrawn()
}

fn withdrawn(store: &Failing, id: u32) -> bool {
    let key = Key::derive(
        &RouteId::new("train"),
        &Identity::new(format!("github.com/{id}/v1.2.0")),
    );
    pollster::block_on(store.inner.entry(&key))
        .unwrap()
        .is_some_and(|kept| kept.value.withdrawn.is_some())
}

#[test]
fn a_member_withdrawn_from_an_announced_group_leaves_its_post_to_the_others_and_the_last_deletes_it()
 {
    let (config, store) = (grouped("", "withdraw = \"delete\""), healthy());
    let script = Script::answering(&[
        POSTED,
        EDITED,
        ("/channels/1566200000000000100/messages/1", 204, ""),
    ]);
    assert_eq!(
        sent(&config, (&store, at(0)), &script, &release("cli", 2)),
        gathered(30)
    );
    assert_eq!(
        sent(&config, (&store, at(1)), &script, &release("core", 1)),
        gathered(30)
    );
    assert_eq!(
        round_outcomes(&swept(&config, (&store, at(30)), &script)),
        [posted()]
    );
    let left = sent(&config, (&store, at(40)), &script, &deleted("cli", 2));
    assert_eq!(unrevised(left), edited());
    let text = crate::grouping::body(&script, 1);
    assert!(text.contains("v1.2.0 of 1"), "{text}");
    assert!(!text.contains("cli with"), "{text}");
    assert!(withdrawn(&store, 2));
    let kept = pollster::block_on(store.inner.pending(&hold()))
        .unwrap()
        .unwrap();
    assert_eq!(kept.value.events.len(), 1);
    assert_eq!(
        sent(&config, (&store, at(41)), &script, &deleted("core", 1)),
        Outcome::Sent {
            steps: vec![Step::Delete(0)]
        }
    );
    assert!(withdrawn(&store, 1));
    let group = Key::derive(&RouteId::new("train"), &group_identity("v1.2.0"));
    let post = pollster::block_on(store.inner.entry(&group))
        .unwrap()
        .unwrap();
    assert_eq!(post.value.withdrawn, Some(at(41)));
    assert_eq!(
        sent(&config, (&store, at(42)), &script, &deleted("core", 1)),
        Outcome::Skipped(super::UNANNOUNCED.to_owned())
    );
}

#[test]
fn a_member_withdrawn_while_its_group_gathers_leaves_its_record_and_nothing_is_sent() {
    let (config, store) = (grouped("", "withdraw = \"delete\""), healthy());
    let script = Script::default();
    assert_eq!(
        sent(&config, (&store, at(0)), &script, &release("cli", 2)),
        gathered(30)
    );
    assert_eq!(
        sent(&config, (&store, at(5)), &script, &deleted("cli", 2)),
        Outcome::Sent { steps: Vec::new() }
    );
    let kept = pollster::block_on(store.inner.pending(&hold()))
        .unwrap()
        .unwrap();
    assert_eq!(kept.value.events, Vec::new());
    assert_eq!(swept(&config, (&store, at(30)), &script), Vec::new());
    assert_eq!(
        pollster::block_on(store.inner.pending(&hold())).unwrap(),
        None
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}

/// The Release of `project`'s `version`, of the repository `id`.
fn versioned(project: &str, id: u32, version: &str) -> herald_core::Event {
    let mut written = serde_json::to_value(release(project, id)).unwrap();
    written["identity"] = format!("github.com/{id}/{version}").into();
    written["attributes"]["version"] = version.into();
    serde_json::from_value(written).unwrap()
}

#[test]
fn a_member_withdrawn_from_one_group_leaves_every_other_group_as_it_was() {
    let (config, store) = (grouped("", "withdraw = \"delete\""), healthy());
    let script = Script::answering(&[
        POSTED,
        POSTED,
        ("/channels/1566200000000000100/messages/1", 204, ""),
    ]);
    let other = versioned("cli", 2, "v1.3.0");
    assert_eq!(
        sent(&config, (&store, at(0)), &script, &release("core", 1)),
        gathered(30)
    );
    sent(&config, (&store, at(0)), &script, &other);
    assert_eq!(
        round_outcomes(&swept(&config, (&store, at(30)), &script)),
        [posted(), posted()]
    );
    let key = Key::derive(&RouteId::new("train"), other.identity());
    let before = pollster::block_on(store.inner.entry(&key))
        .unwrap()
        .unwrap();
    assert_eq!(
        sent(&config, (&store, at(40)), &script, &deleted("core", 1)),
        Outcome::Sent {
            steps: vec![Step::Delete(0)]
        }
    );
    let after = pollster::block_on(store.inner.entry(&key))
        .unwrap()
        .unwrap();
    assert_eq!(after, before);
}

/// A group of `cli` and `core` announced at the end of its window.
fn announced_group(config: &herald_config::Config, store: &Failing, script: &Script) {
    sent(config, (store, at(0)), script, &release("cli", 2));
    sent(config, (store, at(1)), script, &release("core", 1));
    assert_eq!(
        round_outcomes(&swept(config, (store, at(30)), script)),
        [posted()]
    );
}

#[test]
fn a_member_whose_group_s_record_is_gone_is_marked_withdrawn_and_its_post_left() {
    let (config, store) = (grouped("", "withdraw = \"delete\""), healthy());
    let script = Script::answering(&[POSTED]);
    announced_group(&config, &store, &script);
    pollster::block_on(store.inner.drop_pending(&hold())).unwrap();
    assert_eq!(
        sent(&config, (&store, at(40)), &script, &deleted("cli", 2)),
        Outcome::Sent { steps: Vec::new() }
    );
    assert!(withdrawn(&store, 2));
    assert_eq!(script.seen().len(), 1);
}

#[test]
fn a_group_record_another_run_keeps_writing_stops_the_withdrawal() {
    let (config, mut store) = (grouped("", "withdraw = \"delete\""), healthy());
    let script = Script::answering(&[POSTED]);
    announced_group(&config, &store, &script);
    store.fault = crate::harness::Fault::PendingConflicts;
    let stopped = sent(&config, (&store, at(40)), &script, &deleted("cli", 2));
    let Outcome::Stopped { reason, .. } = &stopped else {
        panic!("{stopped:?}");
    };
    assert_eq!(
        reason,
        &herald_flows::Stop::Store("another run wrote the group v1.2.0 first".to_owned())
    );
    assert!(!withdrawn(&store, 2));
}

#[test]
fn a_withdrawal_stopped_once_the_group_s_post_was_edited_finishes_without_editing_it_again() {
    let (config, mut store) = (grouped("", "withdraw = \"delete\""), healthy());
    let script = Script::answering(&[POSTED, EDITED]);
    announced_group(&config, &store, &script);
    store.fault = crate::harness::Fault::GroupedConflicts;
    let stopped = sent(&config, (&store, at(40)), &script, &deleted("cli", 2));
    assert!(matches!(stopped, Outcome::Stopped { .. }), "{stopped:?}");
    assert!(!withdrawn(&store, 2));
    store.fault = crate::harness::Fault::Nothing;
    assert_eq!(
        sent(&config, (&store, at(41)), &script, &deleted("cli", 2)),
        Outcome::Sent { steps: Vec::new() }
    );
    assert!(withdrawn(&store, 2));
    assert_eq!(script.seen().len(), 2);
}

#[test]
fn a_group_whose_records_or_post_cannot_be_read_or_written_stops_the_withdrawal() {
    use crate::harness::Fault;
    for fault in [
        Fault::ReadsPending,
        Fault::ReadsGroups,
        Fault::WritesGroups,
        Fault::ReadsTwice,
    ] {
        let (config, mut store) = (grouped("", "withdraw = \"delete\""), healthy());
        let script = Script::answering(&[POSTED]);
        announced_group(&config, &store, &script);
        store.fault = fault;
        *store.landings.lock().unwrap() = 0;
        let stopped = sent(&config, (&store, at(40)), &script, &deleted("cli", 2));
        let Outcome::Stopped { reason, .. } = &stopped else {
            panic!("{fault:?} {stopped:?}");
        };
        assert_eq!(
            reason,
            &herald_flows::Stop::Store("the store could not be reached: broken".to_owned()),
            "{fault:?}"
        );
        assert!(!withdrawn(&store, 2), "{fault:?}");
    }
}
