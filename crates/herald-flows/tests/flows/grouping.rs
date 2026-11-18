use herald_config::{Config, Uses, load};
use herald_core::{
    Attribute, Draft, Event, Hold, Key, Kind, RouteId, SIGNAL, Signal, SourceName, Step,
    group_identity,
};
use herald_flows::{Budget, Outcome, Ports, Stop, Sweep, Swept, announce, announce_held, prepare};
use herald_ports::{Announcements, Ledger, Waiting};
use herald_testing::{FakeClock, FakeSigner};
use jiff::{SignedDuration, Timestamp};

use crate::harness::*;

mod edits;
mod mentions;
mod rounds;
mod visibility;

pub(crate) const EDITED: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages/1",
    200,
    r#"{"id":"1"}"#,
);
pub(crate) const SECOND: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages",
    200,
    r#"{"id":"2"}"#,
);

/// A config whose route `train` groups by version as `group` adds to its
/// `group` table, its `extra` keys beside it.
pub(crate) fn grouped(group: &str, extra: &str) -> Config {
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
id = "train"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "train"
group = {{ key = "{{% if event.project != 'solo' %}}{{{{ event.attributes.version }}}}{{% endif %}}", lead = ["core"]{group} }}
{extra}
[templates.train]
title = "{{% if group %}}{{{{ group.key }}}} of {{{{ events | length }}}}{{% else %}}{{{{ event.project }}}} alone{{% endif %}}"
body = "{{% for member in events %}}{{{{ member.project }}}} with {{{{ member.body[:20] }}}} and {{% endfor %}}"
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

/// The event of `project`'s `version`, of the repository `id`, as `signal`
/// announces it with the notes `body`.
pub(crate) fn member(project: &str, id: u32, signal: Signal, body: &str) -> Event {
    let mut attributes = herald_core::versioned("v1.2.0".to_owned(), false);
    attributes.insert(SIGNAL.to_owned(), Attribute::Text(signal.name().to_owned()));
    Event::normalise(Draft {
        identity: format!("github.com/{id}/v1.2.0"),
        kind: Kind::RELEASE,
        project: project.to_owned(),
        attributes,
        title: format!("{project} v1.2.0"),
        body: body.to_owned(),
        links: [("release".to_owned(), "https://example.org/r".to_owned())].into(),
        occurred: NOON,
    })
    .unwrap()
}

pub(crate) fn release(project: &str, id: u32) -> Event {
    member(project, id, Signal::Release, "notes")
}

pub(crate) fn at(minutes: i64) -> Timestamp {
    NOON + SignedDuration::from_mins(minutes)
}

/// What `config`'s routes did with `event`, announced at `when` through
/// `script`.
pub(crate) fn sent(
    config: &Config,
    (store, when): (&Failing, Timestamp),
    script: &Script,
    event: &Event,
) -> Outcome {
    let prepared =
        pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(when),
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
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

/// What a round at `when` did with what is held.
pub(crate) fn swept(
    config: &Config,
    (store, when): (&Failing, Timestamp),
    script: &Script,
) -> Vec<Swept> {
    let prepared =
        pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(when),
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    pollster::block_on(announce_held(config, &connected, &ports, "r2")).unwrap()
}

pub(crate) fn hold() -> Hold {
    Hold::Group {
        route: RouteId::new("train"),
        key: "v1.2.0".to_owned(),
    }
}

pub(crate) fn gathered(minutes: i64) -> Outcome {
    Outcome::Gathered {
        key: "v1.2.0".to_owned(),
        due: at(minutes),
    }
}

pub(crate) fn posted() -> Outcome {
    Outcome::Sent {
        steps: vec![Step::Post],
    }
}

pub(crate) fn edited() -> Outcome {
    Outcome::Sent {
        steps: vec![Step::Edit {
            revision: String::new(),
            part: 0,
        }],
    }
}

/// `outcome`, an edit's revision left out.
pub(crate) fn unrevised(outcome: Outcome) -> Outcome {
    match outcome {
        Outcome::Sent { steps } => Outcome::Sent {
            steps: steps
                .into_iter()
                .map(|step| match step {
                    Step::Edit { part, .. } => Step::Edit {
                        revision: String::new(),
                        part,
                    },
                    other => other,
                })
                .collect(),
        },
        other => other,
    }
}

/// The text of the `index`th request `script` sent.
pub(crate) fn body(script: &Script, index: usize) -> String {
    String::from_utf8(script.bodies()[index].clone()).unwrap()
}

/// What a round's sweep announced, each route's outcome.
pub(crate) fn round_outcomes(swept: &[Swept]) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    for swept in swept {
        let Sweep::Released { announced, .. } = &swept.outcome else {
            panic!("{swept:?}");
        };
        for fresh in announced {
            outcomes.extend(fresh.announced.iter().map(|done| done.outcome.clone()));
        }
    }
    outcomes
}

fn member_entry(store: &Failing, event: &Event) -> Option<herald_core::Entry> {
    let key = Key::derive(&RouteId::new("train"), event.identity());
    pollster::block_on(store.inner.entry(&key))
        .unwrap()
        .map(|kept| kept.value)
}

#[test]
fn members_gather_until_the_window_passes_then_go_as_one_post_led_by_the_lead() {
    let (config, store) = (grouped("", ""), healthy());
    let script = Script::answering(&[POSTED]);
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("cli", 2)),
        gathered(30)
    );
    assert_eq!(
        sent(&config, (&store, at(10)), &script, &release("core", 1)),
        gathered(30)
    );
    let kept = pollster::block_on(store.inner.pending(&hold()))
        .unwrap()
        .unwrap();
    assert_eq!(kept.value.events.len(), 2);
    assert_eq!((kept.value.opened, kept.value.due), (NOON, at(30)));
    assert_eq!(
        swept(&config, (&store, at(29)), &script),
        Vec::<Swept>::new()
    );
    assert_eq!(script.seen(), Vec::<String>::new());
    let swept = swept(&config, (&store, at(30)), &script);
    assert_eq!(round_outcomes(&swept), [posted()]);
    assert_eq!(swept[0].source, SourceName::new("gate"));
    let text = body(&script, 0);
    assert!(text.contains("v1.2.0 of 2"), "{text}");
    assert!(
        text.contains("core with notes and cli with notes and"),
        "{text}"
    );
    let kept = pollster::block_on(store.inner.pending(&hold()))
        .unwrap()
        .unwrap();
    assert_eq!(kept.value.closed, Some(at(30)));
    assert!(kept.value.landed);
    for event in [release("cli", 2), release("core", 1)] {
        let entry = member_entry(&store, &event).unwrap();
        assert_eq!(entry.group.as_deref(), Some("v1.2.0"));
        assert_eq!(entry.revision, Some(herald_core::shown(&event)));
        let latest = pollster::block_on(store.inner.latest(event.project()))
            .unwrap()
            .unwrap();
        assert_eq!(latest.value.newest.title, event.title());
        assert!(latest.value.routes.contains(&RouteId::new("train")));
    }
    let group = Key::derive(&RouteId::new("train"), &group_identity("v1.2.0"));
    assert!(
        pollster::block_on(store.inner.entry(&group))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        sent(&config, (&store, at(31)), &script, &release("core", 1)),
        Outcome::AlreadyAnnounced
    );
    assert!(swept_again_sends_nothing(&config, &store, &script));
}

fn swept_again_sends_nothing(config: &Config, store: &Failing, script: &Script) -> bool {
    let before = script.seen().len();
    swept(config, (store, at(60)), script).is_empty() && script.seen().len() == before
}

#[test]
fn a_held_group_waiting_for_its_lead_goes_as_soon_as_the_lead_comes() {
    let (config, store) = (grouped(", until = \"lead\"", ""), healthy());
    let script = Script::answering(&[POSTED]);
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("cli", 2)),
        gathered(30)
    );
    assert_eq!(
        sent(&config, (&store, at(5)), &script, &release("core", 1)),
        posted()
    );
    assert!(body(&script, 0).contains("core with notes and cli with notes and"));
}

#[test]
fn a_member_coming_once_its_group_is_due_but_not_yet_announced_goes_with_it_at_once() {
    let (config, store) = (grouped("", ""), healthy());
    let script = Script::answering(&[POSTED]);
    sent(&config, (&store, NOON), &script, &release("cli", 2));
    assert_eq!(
        sent(&config, (&store, at(45)), &script, &release("web", 3)),
        posted()
    );
    assert!(body(&script, 0).contains("cli with notes and web with notes and"));
}

#[test]
fn an_event_whose_key_names_no_group_is_announced_alone() {
    let (config, store) = (grouped("", ""), healthy());
    let script = Script::answering(&[POSTED]);
    let solo = release("solo", 9);
    assert_eq!(sent(&config, (&store, NOON), &script, &solo), posted());
    assert!(body(&script, 0).contains("solo alone"));
    let latest = pollster::block_on(store.inner.latest("solo"))
        .unwrap()
        .unwrap();
    assert_eq!(latest.value.newest.title, solo.title());
    assert_eq!(
        sent(&config, (&store, NOON), &script, &solo),
        Outcome::AlreadyAnnounced
    );
    let alone = Key::derive(&RouteId::new("train"), solo.identity());
    let entry = pollster::block_on(store.inner.entry(&alone))
        .unwrap()
        .unwrap();
    assert_eq!(entry.value.group, None);
}

#[test]
fn a_member_past_the_group_s_most_is_announced_alone() {
    let (config, store) = (grouped(", most = 1", ""), healthy());
    let script = Script::answering(&[POSTED]);
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("cli", 2)),
        gathered(30)
    );
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("web", 3)),
        posted()
    );
    assert!(body(&script, 0).contains("web alone"));
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("cli", 2)),
        gathered(30)
    );
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("web", 3)),
        Outcome::AlreadyAnnounced
    );
}

#[test]
fn a_member_the_store_s_record_cannot_hold_is_announced_alone() {
    let mut config = grouped("", "");
    let deno = load(
        "herald.toml",
        "version = 1\n[store]\nkind = \"deno-kv\"\n",
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap();
    config.store = deno.store;
    let store = healthy();
    let script = Script::answering(&[POSTED]);
    let long = member("long", 4, Signal::Release, &"n".repeat(70_000));
    assert_eq!(sent(&config, (&store, NOON), &script, &long), posted());
    assert_eq!(
        sent(&config, (&store, NOON), &script, &release("cli", 2)),
        gathered(30)
    );
}

#[test]
fn an_edit_of_a_member_no_group_holds_announces_nothing_and_one_held_takes_its_place() {
    let (config, store) = (grouped("", ""), healthy());
    let script = Script::answering(&[POSTED]);
    let edit = member("core", 1, Signal::Release, "fixed").marked_edited();
    assert_eq!(
        sent(&config, (&store, NOON), &script, &edit),
        Outcome::Unedited
    );
    sent(&config, (&store, NOON), &script, &release("cli", 2));
    assert_eq!(
        sent(&config, (&store, NOON), &script, &edit),
        Outcome::Unedited
    );
    sent(&config, (&store, NOON), &script, &release("core", 1));
    assert_eq!(sent(&config, (&store, NOON), &script, &edit), gathered(30));
    assert_eq!(
        round_outcomes(&swept(&config, (&store, at(30)), &script)),
        [posted()]
    );
    assert!(body(&script, 0).contains("core with fixed and cli with notes"));
}

#[test]
fn a_group_whose_record_or_ledger_cannot_be_read_or_written_stops_its_route() {
    let config = grouped("", "");
    let script = Script::default();
    let event = release("cli", 2);
    let store = |fault| Failing { fault, ..healthy() };
    for fault in [
        Fault::ReadsHeld,
        Fault::WritesPending,
        Fault::Reads,
        Fault::ReadsOnce,
        Fault::ReadsTwice,
    ] {
        let Outcome::Stopped { reason, .. } = sent(&config, (&store(fault), NOON), &script, &event)
        else {
            panic!("{fault:?}");
        };
        assert_eq!(
            reason,
            Stop::Store("the store could not be reached: broken".to_owned()),
            "{fault:?}"
        );
    }
    let Outcome::Stopped {
        reason: Stop::Store(unkept),
        ..
    } = sent(
        &config,
        (&store(Fault::KeepsNoPending), NOON),
        &script,
        &event,
    )
    else {
        panic!("a store that keeps no held events stops a route that groups");
    };
    assert!(unkept.contains("held events"), "{unkept}");
    assert_eq!(
        sent(
            &config,
            (&store(Fault::PendingConflicts), NOON),
            &script,
            &event
        ),
        Outcome::Stopped {
            landed: Vec::new(),
            remaining: Vec::new(),
            reason: Stop::Store("the group v1.2.0 changed under each of 3 writes".to_owned()),
        }
    );
}

#[test]
fn a_member_whose_key_does_not_render_stops_its_route() {
    let config = grouped("", "");
    let entry = Event::normalise(Draft {
        identity: "feed/entry".to_owned(),
        kind: Kind::FEED_ENTRY,
        project: "cli".to_owned(),
        attributes: std::collections::BTreeMap::new(),
        title: "entry".to_owned(),
        body: String::new(),
        links: [("entry".to_owned(), "https://example.org/e".to_owned())].into(),
        occurred: NOON,
    })
    .unwrap();
    let Outcome::Stopped { reason, .. } =
        sent(&config, (&healthy(), NOON), &Script::default(), &entry)
    else {
        panic!("a key that does not render stops the route");
    };
    assert!(matches!(reason, Stop::Template(_)), "{reason:?}");
}
