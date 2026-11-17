//! A release the forge deleted, withdrawn from each route as its `withdraw`
//! says: every message herald recorded deleted, the thread archived, the
//! pin forgotten, what its project last announced falling back, and a
//! re-created release announced anew (ADR-0048).

use herald_config::{Config, load};
use herald_core::{Chore, Entry, Event, Facts, Hold, Identity, Key, RouteId, Step};
use herald_flows::{Outcome, Stop, Sweep, Swept};
use herald_ports::{Announcements, Ledger, Pins, Schedules, Waiting};
use jiff::Timestamp;

use crate::grouping::{sent, swept};
use crate::harness::{Failing, Fault, NOON, POSTED, Script, event, healthy};

mod activitypub;
mod feed;
mod groups;
mod operator;

/// Why a route keeps the post of a release the forge deleted.
pub(crate) const KEPT: &str =
    "the route keeps a deleted release's post, as its withdraw = \"keep\" says";
/// Why a withdrawal sends nothing: the route announced nothing of it.
pub(crate) const UNANNOUNCED: &str = "the route announced nothing of the release";
/// The deletion of the post the first announcement posted.
pub(crate) const DELETED: (&str, u16, &str) = ("/channels/1566200000000000100/messages/1", 204, "");

/// A config whose route `releases` withdraws as `withdraw` says, its
/// `extra` keys beside it.
pub(crate) fn withdrawing(withdraw: &str, extra: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "release"
withdraw = "{withdraw}"
{extra}
[templates.release]
title = "{{{{ event.project }}}} {{{{ event.attributes.version }}}}"
body = "{{{{ event.body }}}}"
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

/// The forge's deletion of the release `version`.
pub(crate) fn deleted(version: &str) -> Event {
    event(version).marked_withdrawn()
}

/// The route's entry of `identity`, where it keeps one.
pub(crate) fn entry(store: &Failing, identity: &str) -> Option<Entry> {
    let key = Key::derive(&RouteId::new("releases"), &Identity::new(identity));
    pollster::block_on(store.inner.entry(&key))
        .unwrap()
        .map(|kept| kept.value)
}

/// The route's entry of `identity`, as `change` makes it of what it was.
pub(crate) fn kept(store: &Failing, identity: &str, change: impl FnOnce(Entry) -> Entry) {
    let key = Key::derive(&RouteId::new("releases"), &Identity::new(identity));
    let earlier = pollster::block_on(store.inner.entry(&key)).unwrap();
    let version = earlier.as_ref().map(|kept| kept.version);
    let entry = earlier.map_or_else(|| Entry::new(key), |kept| kept.value);
    let written = pollster::block_on(store.inner.put_entry(&change(entry), version)).unwrap();
    assert!(matches!(written, herald_ports::Written::Written(_)));
}

/// What landed as a message with the id `id`.
pub(crate) fn message(id: &str) -> Facts {
    Facts {
        message: Some(id.to_owned()),
        ..Facts::default()
    }
}

/// The withdrawal record of the release `version` on the route.
pub(crate) fn hold(version: &str) -> Hold {
    Hold::Withdraw {
        route: RouteId::new("releases"),
        identity: Identity::new(format!("github.com/1/{version}")),
    }
}

/// Whether the store keeps the withdrawal record of `version`.
pub(crate) fn holding(store: &Failing, version: &str) -> bool {
    pollster::block_on(store.inner.pending(&hold(version)))
        .unwrap()
        .is_some()
}

fn withdrew(steps: &[Step]) -> Outcome {
    Outcome::Sent {
        steps: steps.to_vec(),
    }
}

fn stop(outcome: &Outcome) -> &Stop {
    match outcome {
        Outcome::Stopped { reason, .. } => reason,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_deleted_release_s_post_is_deleted_and_a_re_created_one_is_announced_anew() {
    let (config, store) = (withdrawing("delete", ""), healthy());
    let script = Script::answering(&[POSTED, DELETED, POSTED, DELETED]);
    let posted = withdrew(&[Step::Post]);
    assert_eq!(sent(&config, (&store, NOON), &script, &event("v1")), posted);
    assert_eq!(
        sent(&config, (&store, NOON), &script, &deleted("v1")),
        withdrew(&[Step::Delete(0)])
    );
    let first = entry(&store, "github.com/1/v1").unwrap();
    assert_eq!(first.withdrawn, Some(NOON));
    assert_eq!(first.message(&Step::Post), None);
    assert!(first.lease.is_none());
    assert!(!holding(&store, "v1"));
    assert_eq!(
        pollster::block_on(store.inner.latest("php-mutation-gate")).unwrap(),
        None
    );
    assert_eq!(sent(&config, (&store, NOON), &script, &event("v1")), posted);
    let again = entry(&store, "github.com/1/v1#1").unwrap();
    assert_eq!(again.message(&Step::Post), Some("1"));
    assert_eq!(
        sent(&config, (&store, NOON), &script, &deleted("v1")),
        withdrew(&[Step::Delete(0)])
    );
    assert_eq!(
        entry(&store, "github.com/1/v1#1").unwrap().withdrawn,
        Some(NOON)
    );
    assert_eq!(
        script.seen(),
        [
            "POST https://discord.com/api/v10/channels/1566200000000000100/messages",
            "DELETE https://discord.com/api/v10/channels/1566200000000000100/messages/1",
            "POST https://discord.com/api/v10/channels/1566200000000000100/messages",
            "DELETE https://discord.com/api/v10/channels/1566200000000000100/messages/1",
        ]
    );
}

#[test]
fn a_route_that_keeps_or_announced_nothing_withdraws_nothing() {
    let store = healthy();
    let script = Script::default();
    assert_eq!(
        sent(
            &withdrawing("keep", ""),
            (&store, NOON),
            &script,
            &deleted("v1")
        ),
        Outcome::Skipped(KEPT.to_owned())
    );
    assert!(!holding(&store, "v1"));
    assert_eq!(
        sent(
            &withdrawing("delete", ""),
            (&store, NOON),
            &script,
            &deleted("v1")
        ),
        Outcome::Skipped(UNANNOUNCED.to_owned())
    );
    assert!(!holding(&store, "v1"));
    assert_eq!(entry(&store, "github.com/1/v1"), None);
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn each_chunk_goes_last_first_then_the_post_the_late_release_s_post_and_the_pin_and_thread() {
    let config = withdrawing(
        "delete",
        "pin = \"store\"\nthread = { name = \"t\", archive_after = \"14d\" }",
    );
    let store = healthy();
    kept(&store, "github.com/1/v1", |entry| {
        entry
            .landed(Step::Post, message("1"))
            .landed(
                Step::OpenThread,
                Facts {
                    thread: Some("7".to_owned()),
                    ..Facts::default()
                },
            )
            .landed(Step::Chunk(1), message("2"))
            .landed(Step::Chunk(2), message("3"))
    });
    kept(&store, "github.com/1/v1#release", |entry| {
        entry.landed(Step::Post, message("4"))
    });
    let route = RouteId::new("releases");
    pollster::block_on(store.inner.put_pinned(&route, "1")).unwrap();
    let script = Script::answering(&[
        ("/channels/7/messages/3", 204, ""),
        ("/channels/7/messages/2", 204, ""),
        DELETED,
        ("/channels/1566200000000000100/messages/4", 404, ""),
    ]);
    assert_eq!(
        sent(&config, (&store, NOON), &script, &deleted("v1")),
        withdrew(&[
            Step::Delete(2),
            Step::Delete(1),
            Step::Delete(0),
            Step::Delete(0)
        ])
    );
    assert_eq!(
        pollster::block_on(store.inner.pinned(&route)).unwrap(),
        None
    );
    let due = pollster::block_on(store.inner.every_schedule()).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!((due[0].chore, due[0].due), (Chore::Archive, NOON));
    assert_eq!(due[0].thread, "7");
    assert_eq!(
        entry(&store, "github.com/1/v1#release").unwrap().withdrawn,
        Some(NOON)
    );
    assert_eq!(
        entry(&store, "github.com/1/v1").unwrap().withdrawn,
        Some(NOON)
    );
}

#[test]
fn a_pin_of_another_post_is_kept_and_a_thread_never_archived_is_not_scheduled() {
    let config = withdrawing(
        "delete",
        "pin = \"store\"\nthread = { name = \"t\", lock_after = \"1d\" }",
    );
    let store = healthy();
    kept(&store, "github.com/1/v1", |entry| {
        entry.landed(
            Step::Post,
            Facts {
                message: Some("1".to_owned()),
                thread: Some("7".to_owned()),
                permalink: None,
            },
        )
    });
    let route = RouteId::new("releases");
    pollster::block_on(store.inner.put_pinned(&route, "9")).unwrap();
    let script = Script::answering(&[DELETED]);
    assert_eq!(
        sent(&config, (&store, NOON), &script, &deleted("v1")),
        withdrew(&[Step::Delete(0)])
    );
    assert_eq!(
        pollster::block_on(store.inner.pinned(&route))
            .unwrap()
            .as_deref(),
        Some("9")
    );
    assert_eq!(
        pollster::block_on(store.inner.every_schedule()).unwrap(),
        []
    );
}

#[test]
fn a_withdrawal_stopped_part_way_is_finished_by_a_round_from_its_record() {
    let (config, store) = (withdrawing("delete", ""), healthy());
    kept(&store, "github.com/1/v1", |entry| {
        entry
            .landed(Step::Post, message("1"))
            .landed(Step::Chunk(1), message("2"))
    });
    let script = Script::answering(&[
        ("/channels/1566200000000000100/messages/2", 204, ""),
        ("/channels/1566200000000000100/messages/1", 500, "{}"),
    ]);
    let stopped = sent(&config, (&store, NOON), &script, &deleted("v1"));
    let Outcome::Stopped { landed, .. } = &stopped else {
        panic!("{stopped:?}");
    };
    assert_eq!(landed, &[Step::Delete(1)]);
    assert!(holding(&store, "v1"));
    let record = pollster::block_on(store.inner.pending(&hold("v1")))
        .unwrap()
        .unwrap();
    assert_eq!(record.value.events[0].event, deleted("v1"));
    assert_eq!(entry(&store, "github.com/1/v1").unwrap().withdrawn, None);
    let later: Timestamp = NOON + jiff::SignedDuration::from_hours(1);
    let script = Script::answering(&[DELETED]);
    let finished = swept(&config, (&store, later), &script);
    assert_eq!(
        finished,
        [Swept {
            source: herald_core::SourceName::new("releases"),
            member: None,
            outcome: Sweep::Withdrawn {
                identity: Identity::new("github.com/1/v1"),
                announced: vec![herald_flows::Announced {
                    route: RouteId::new("releases"),
                    outcome: withdrew(&[Step::Delete(0)]),
                }],
            },
        }]
    );
    assert!(!holding(&store, "v1"));
    assert_eq!(
        entry(&store, "github.com/1/v1").unwrap().withdrawn,
        Some(later)
    );
    assert_eq!(swept(&config, (&store, later), &script), []);
}

#[test]
fn an_announcement_another_run_holds_is_left_to_it() {
    let (config, store) = (withdrawing("delete", ""), healthy());
    kept(&store, "github.com/1/v1", |entry| {
        entry.landed(Step::Post, message("1")).claimed_by(
            "r0",
            NOON,
            NOON + jiff::SignedDuration::from_mins(5),
        )
    });
    let held = sent(&config, (&store, NOON), &Script::default(), &deleted("v1"));
    assert_eq!(
        held,
        Outcome::Held {
            by: "r0".to_owned(),
            until: NOON + jiff::SignedDuration::from_mins(5),
        }
    );
    assert!(holding(&store, "v1"));
}

#[test]
fn what_the_project_last_announced_falls_back_to_its_stable_release() {
    let (config, store) = (withdrawing("delete", ""), healthy());
    let script = Script::answering(&[POSTED, POSTED, DELETED]);
    let candidate = Event::normalise(herald_core::Draft {
        identity: "github.com/1/v2.0.0-rc.1".to_owned(),
        kind: herald_core::Kind::RELEASE,
        project: "php-mutation-gate".to_owned(),
        attributes: herald_core::versioned("v2.0.0-rc.1".to_owned(), true),
        title: "php-mutation-gate".to_owned(),
        body: String::new(),
        links: [("release".to_owned(), "https://example.org/r".to_owned())].into(),
        occurred: NOON,
    })
    .unwrap();
    sent(&config, (&store, NOON), &script, &event("v1"));
    sent(&config, (&store, NOON), &script, &candidate);
    let withdrawn = candidate.clone().marked_withdrawn();
    assert_eq!(
        sent(&config, (&store, NOON), &script, &withdrawn),
        withdrew(&[Step::Delete(0)])
    );
    let latest = pollster::block_on(store.inner.latest("php-mutation-gate"))
        .unwrap()
        .unwrap();
    assert_eq!(latest.value.newest.version(), Some("v1"));
    assert!(latest.value.shows(event("v1").identity()));
}

#[test]
fn what_a_project_last_announced_is_left_where_it_shows_another_release_or_is_not_kept() {
    let (config, store) = (withdrawing("delete", ""), healthy());
    let script = Script::answering(&[POSTED, DELETED]);
    sent(&config, (&store, NOON), &script, &event("v1"));
    kept(&store, "github.com/1/v2", |entry| {
        entry.landed(Step::Post, message("1"))
    });
    assert_eq!(
        sent(&config, (&store, NOON), &script, &deleted("v2")),
        withdrew(&[Step::Delete(0)])
    );
    let latest = pollster::block_on(store.inner.latest("php-mutation-gate"))
        .unwrap()
        .unwrap();
    assert!(latest.value.shows(event("v1").identity()));
    let mut unkept = healthy();
    unkept.fault = Fault::KeepsNoLatest;
    kept(&unkept, "github.com/1/v1", |entry| {
        entry.landed(Step::Post, message("1"))
    });
    let script = Script::answering(&[DELETED]);
    assert_eq!(
        sent(&config, (&unkept, NOON), &script, &deleted("v1")),
        withdrew(&[Step::Delete(0)])
    );
}

#[test]
fn a_store_that_cannot_keep_the_record_or_forget_stops_the_route() {
    for (fault, landed) in [
        (Fault::KeepsNoPending, false),
        (Fault::ReadsHeld, false),
        (Fault::Reads, false),
        (Fault::ReadsOnce, false),
        (Fault::ForgetsPins, true),
        (Fault::ReadsLatest, true),
        (Fault::DropsPending, true),
    ] {
        let config = withdrawing("delete", "pin = \"store\"");
        let mut store = healthy();
        kept(&store, "github.com/1/v1", |entry| {
            entry.landed(Step::Post, message("1"))
        });
        pollster::block_on(store.inner.put_pinned(&RouteId::new("releases"), "1")).unwrap();
        store.fault = fault;
        let script = Script::answering(&[DELETED]);
        let stopped = sent(&config, (&store, NOON), &script, &deleted("v1"));
        assert!(
            matches!(stop(&stopped), Stop::Store(_)),
            "{fault:?} {stopped:?}"
        );
        assert_eq!(script.seen().len(), usize::from(landed), "{fault:?}");
    }
}
