use herald_config::{Config, Source, Uses};
use herald_core::{Hold, Identity, Signal, SourceName};
use herald_flows::{Budget, Outcome, Ports, Stop, Sweep, Swept, poll, prepare};
use herald_ports::Waiting;
use herald_sources_forge::ForgeSource;
use herald_testing::{FakeClock, FakeSigner};
use jiff::{SignedDuration, Timestamp};

use crate::harness::*;

const RELEASES: &str = "/repos/nightworksio/php-mutation-gate/releases?per_page=30";
const TAGS: &str = "/repos/nightworksio/php-mutation-gate/tags?per_page=30";
const REPOSITORY: &str = "/repos/nightworksio/php-mutation-gate";
const PAGE: &str = r#"{"id":1,"name":"php-mutation-gate","visibility":"public","html_url":"https://github.com/nightworksio/php-mutation-gate"}"#;
const TAGGED: &str = r#"[{"name":"v1"}]"#;

fn tagged(prefer: Option<herald_core::Prefer>) -> Config {
    let mut config = config("");
    if let Some(source) = config.sources.values_mut().next()
        && let Some(mut forged) = source.of::<ForgeSource>().cloned()
    {
        forged.options.signals = Some(vec![Signal::Release, Signal::Tag]);
        forged.options.prefer = prefer;
        *source = Source::new(forged);
    }
    config
}

fn round(config: &Config, store: &Failing, script: &Script, at: Timestamp) -> Vec<Swept> {
    let prepared =
        pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(at);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    pollster::block_on(poll(
        config,
        &connected,
        &ports,
        &herald_testing::FakeVerifier,
        "r1",
    ))
}

fn release(version: &str) -> String {
    serde_json::json!([{
        "tag_name": version,
        "name": format!("gate {version}"),
        "body": "## Added",
        "draft": false,
        "prerelease": false,
        "published_at": NOON.to_string(),
        "html_url": format!("https://github.com/nightworksio/php-mutation-gate/releases/tag/{version}"),
    }])
    .to_string()
}

fn hold() -> Hold {
    Hold::Tag {
        source: SourceName::new("gate"),
        identity: Identity::new("github.com/1/v1"),
    }
}

/// A first round, which remembers no tag, then one finding the tag `v1`.
fn found_v1(config: &Config, store: &Failing) -> Vec<Swept> {
    let first = Script::answering(&[
        (RELEASES, 200, "[]"),
        (TAGS, 200, "[]"),
        (REPOSITORY, 200, PAGE),
    ]);
    round(config, store, &first, NOON);
    let commit = serde_json::json!({"committer": {"date": NOON.to_string()}}).to_string();
    let tag = Script::answering(&[
        (RELEASES, 200, "[]"),
        (TAGS, 200, TAGGED),
        (REPOSITORY, 200, PAGE),
        (
            "/repos/nightworksio/php-mutation-gate/git/ref/tags/v1",
            200,
            r#"{"object":{"type":"commit","sha":"c1"}}"#,
        ),
        (
            "/repos/nightworksio/php-mutation-gate/git/commits/c1",
            200,
            &commit,
        ),
    ]);
    round(config, store, &tag, NOON)
}

fn announced_steps(swept: &Sweep) -> Vec<bool> {
    let fresh = match swept {
        Sweep::Polled { announced, .. } | Sweep::Released { announced, .. } => announced,
        other => panic!("{other:?}"),
    };
    fresh
        .iter()
        .flat_map(|fresh| &fresh.announced)
        .map(|announced| matches!(announced.outcome, Outcome::Sent { .. }))
        .collect()
}

#[test]
fn a_tag_without_its_release_is_held_for_its_window_then_announced() {
    let (config, store) = (tagged(None), healthy());
    let swept = found_v1(&config, &store);
    let Sweep::Polled { announced, .. } = &swept[0].outcome else {
        panic!("{swept:?}");
    };
    let due = NOON + SignedDuration::from_hours(1);
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].held, Some(due));
    assert_eq!(announced[0].announced, Vec::new());
    let kept = pollster::block_on(store.inner.pending(&hold()))
        .unwrap()
        .unwrap();
    assert_eq!((kept.value.opened, kept.value.due), (NOON, due));
    let quiet = || {
        Script::answering(&[
            (RELEASES, 200, "[]"),
            (TAGS, 200, TAGGED),
            (REPOSITORY, 200, PAGE),
        ])
    };
    let waiting = round(
        &config,
        &store,
        &quiet(),
        NOON + SignedDuration::from_mins(30),
    );
    assert_eq!(waiting.len(), 1);
    let mut answers = vec![POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS];
    answers.extend([
        (RELEASES, 200, "[]"),
        (TAGS, 200, TAGGED),
        (REPOSITORY, 200, PAGE),
    ]);
    let later = round(&config, &store, &Script::answering(&answers), due);
    assert_eq!(later[0].source, SourceName::new("gate"));
    assert_eq!(announced_steps(&later[0].outcome), [true]);
    assert!(matches!(
        later[0].outcome,
        Sweep::Released { withheld: None, .. }
    ));
    assert_eq!(
        pollster::block_on(store.inner.pending(&hold())).unwrap(),
        None
    );
}

#[test]
fn a_release_inside_the_window_is_announced_and_lets_its_tag_go() {
    let (config, store) = (tagged(None), healthy());
    found_v1(&config, &store);
    let release = release("v1");
    let mut answers = vec![
        (RELEASES, 200, release.as_str()),
        (TAGS, 200, TAGGED),
        (REPOSITORY, 200, PAGE),
    ];
    answers.extend([POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]);
    let released = round(
        &config,
        &store,
        &Script::answering(&answers),
        NOON + SignedDuration::from_mins(10),
    );
    assert_eq!(announced_steps(&released[0].outcome), [true]);
    assert_eq!(
        pollster::block_on(store.inner.pending(&hold())).unwrap(),
        None
    );
}

#[test]
fn a_source_preferring_whichever_comes_first_announces_its_tag_at_once() {
    let (config, store) = (tagged(Some(herald_core::Prefer::First)), healthy());
    let first = Script::answering(&[
        (RELEASES, 200, "[]"),
        (TAGS, 200, "[]"),
        (REPOSITORY, 200, PAGE),
    ]);
    round(&config, &store, &first, NOON);
    let commit = serde_json::json!({"committer": {"date": NOON.to_string()}}).to_string();
    let mut answers = vec![
        (RELEASES, 200, "[]"),
        (TAGS, 200, TAGGED),
        (REPOSITORY, 200, PAGE),
        (
            "/repos/nightworksio/php-mutation-gate/git/ref/tags/v1",
            200,
            r#"{"object":{"type":"commit","sha":"c1"}}"#,
        ),
        (
            "/repos/nightworksio/php-mutation-gate/git/commits/c1",
            200,
            commit.as_str(),
        ),
    ];
    answers.extend([POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]);
    let swept = round(&config, &store, &Script::answering(&answers), NOON);
    assert_eq!(announced_steps(&swept[0].outcome), [true]);
    assert_eq!(
        pollster::block_on(store.inner.pending(&hold())).unwrap(),
        None
    );
}

#[test]
fn held_events_that_cannot_be_read_or_kept_stop_the_source() {
    let config = tagged(None);
    let unreadable = Failing {
        fault: Fault::ReadsPending,
        ..healthy()
    };
    let swept = round(
        &config,
        &unreadable,
        &Script::answering(&[
            (RELEASES, 200, "[]"),
            (TAGS, 200, "[]"),
            (REPOSITORY, 200, PAGE),
        ]),
        NOON,
    );
    assert_eq!(swept[0].source, SourceName::new("gate"));
    assert!(matches!(&swept[0].outcome, Sweep::Stopped(Stop::Store(_))));
    assert_eq!(swept.len(), 2);
    let unwritable = Failing {
        fault: Fault::WritesPending,
        ..healthy()
    };
    let swept = found_v1(&config, &unwritable);
    assert!(matches!(&swept[0].outcome, Sweep::Stopped(Stop::Store(_))));
}

fn pending(member: Option<&str>, due: Timestamp) -> herald_core::Pending {
    let event = herald_core::Event::normalise(herald_core::Draft {
        identity: "github.com/1/v1".to_owned(),
        kind: herald_core::Kind::RELEASE,
        project: "php-mutation-gate".to_owned(),
        title: "v1".to_owned(),
        body: String::new(),
        links: std::collections::BTreeMap::new(),
        attributes: herald_core::versioned("v1", false),
        occurred: NOON,
    })
    .unwrap();
    herald_core::Pending {
        hold: hold(),
        events: vec![herald_core::HeldEvent {
            source: SourceName::new("gate"),
            member: member.map(ToOwned::to_owned),
            event,
        }],
        opened: NOON,
        due,
        closed: None,
        landed: false,
    }
}

#[test]
fn a_tag_already_held_keeps_its_time_and_a_route_that_stops_keeps_it_held() {
    let (config, store) = (tagged(None), healthy());
    let soon = NOON + SignedDuration::from_mins(5);
    pollster::block_on(store.inner.put_pending(&pending(None, soon), None)).unwrap();
    let swept = found_v1(&config, &store);
    let Sweep::Polled { announced, .. } = &swept[0].outcome else {
        panic!("{swept:?}");
    };
    assert_eq!(announced[0].held, Some(soon));
    let refused = Script::answering(&[
        (
            "/channels/1566200000000000100/messages",
            403,
            r#"{"code":50013,"message":"Missing Permissions"}"#,
        ),
        (RELEASES, 200, "[]"),
        (TAGS, 200, TAGGED),
        (REPOSITORY, 200, PAGE),
    ]);
    let stopped = round(&config, &store, &refused, soon);
    assert_eq!(announced_steps(&stopped[0].outcome), [false]);
    assert!(
        pollster::block_on(store.inner.pending(&hold()))
            .unwrap()
            .is_some()
    );
}

#[test]
fn held_events_that_cannot_be_dropped_say_so_and_one_of_an_unlisted_member_is_let_go() {
    let config = tagged(None);
    let stuck = Failing {
        fault: Fault::DropsPending,
        ..healthy()
    };
    pollster::block_on(stuck.inner.put_pending(&pending(Some("9"), NOON), None)).unwrap();
    let swept = round(
        &config,
        &stuck,
        &Script::answering(&[
            (RELEASES, 200, "[]"),
            (TAGS, 200, "[]"),
            (REPOSITORY, 200, PAGE),
        ]),
        NOON,
    );
    assert_eq!(
        swept[0].outcome,
        Sweep::Released {
            announced: Vec::new(),
            withheld: Some(
                "the owner's last membership record no longer holds the member".to_owned()
            ),
        }
    );
    assert_eq!(swept[1].source, SourceName::new("gate"));
    assert!(matches!(&swept[1].outcome, Sweep::Stopped(Stop::Store(_))));
    let unread = Failing {
        fault: Fault::ReadsMembers,
        ..healthy()
    };
    pollster::block_on(unread.inner.put_pending(&pending(Some("9"), NOON), None)).unwrap();
    let swept = round(
        &config,
        &unread,
        &Script::answering(&[
            (RELEASES, 200, "[]"),
            (TAGS, 200, "[]"),
            (REPOSITORY, 200, PAGE),
        ]),
        NOON,
    );
    assert!(matches!(&swept[0].outcome, Sweep::Stopped(Stop::Store(_))));
    assert!(
        pollster::block_on(unread.inner.pending(&hold()))
            .unwrap()
            .is_some()
    );
}

/// A store holding the member `9`, `nightworksio/plugins`, which left at
/// `left` where it did, and a tag of it held until noon.
fn held_of_member(left: Option<Timestamp>) -> Failing {
    use herald_ports::Cursors;
    let store = healthy();
    let membership = herald_core::Membership {
        repository: herald_core::Listed {
            id: "9".to_owned(),
            name: "plugins".to_owned(),
            path: "nightworksio/plugins".to_owned(),
            topics: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            visibility: herald_core::Visibility::Public,
            archived: false,
            fork: false,
            template: false,
            language: None,
            linked: None,
        },
        discovered: NOON,
        left,
        cursor: None,
        collides: None,
        hooked: false,
    };
    pollster::block_on(
        store
            .inner
            .put_member(&SourceName::new("gate"), &membership, None),
    )
    .unwrap();
    pollster::block_on(store.inner.put_pending(&pending(Some("9"), NOON), None)).unwrap();
    store
}

#[test]
fn a_held_tag_of_a_member_still_listed_is_announced_as_its_and_one_of_a_member_that_left_is_let_go()
{
    let config = tagged(None);
    let quiet = [
        (RELEASES, 200, "[]"),
        (TAGS, 200, "[]"),
        (REPOSITORY, 200, PAGE),
    ];
    let listed = held_of_member(None);
    let mut answers = vec![POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS];
    answers.extend(quiet);
    let swept = round(&config, &listed, &Script::answering(&answers), NOON);
    assert_eq!(swept[0].member.as_deref(), Some("nightworksio/plugins"));
    assert_eq!(announced_steps(&swept[0].outcome), [true]);
    let gone = held_of_member(Some(NOON));
    let swept = round(&config, &gone, &Script::answering(&quiet), NOON);
    assert_eq!(swept[0].member, None);
    assert_eq!(
        swept[0].outcome,
        Sweep::Released {
            announced: Vec::new(),
            withheld: Some(
                "the owner's last membership record no longer holds the member".to_owned()
            ),
        }
    );
    assert_eq!(
        pollster::block_on(gone.inner.pending(&hold())).unwrap(),
        None
    );
}

#[test]
fn a_store_keeping_no_held_events_releases_none() {
    let store = Failing {
        fault: Fault::KeepsNoPending,
        ..healthy()
    };
    let swept = round(
        &config(""),
        &store,
        &Script::answering(&[(RELEASES, 200, "[]"), (REPOSITORY, 200, PAGE)]),
        NOON,
    );
    assert_eq!(swept.len(), 1);
    assert!(
        matches!(swept[0].outcome, Sweep::Polled { .. }),
        "{swept:?}"
    );
}

#[test]
fn housekeeping_announces_a_held_tag_that_fell_due() {
    let (config, store) = (tagged(None), healthy());
    pollster::block_on(store.inner.put_pending(&pending(None, NOON), None)).unwrap();
    let script = Script::answering(&[POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]);
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let keyring = herald_records::MemoryKeyring::new(8);
    let housekept = pollster::block_on(herald_flows::housekeep_announcing(
        (&config, &connected, &ports),
        &keys_in(&keyring),
        "r1",
    ))
    .unwrap();
    assert_eq!(housekept.released.len(), 1);
    assert_eq!(announced_steps(&housekept.released[0].outcome), [true]);
    let unreadable = Failing {
        fault: Fault::ReadsPending,
        ..healthy()
    };
    let ports = Ports {
        store: &unreadable,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    assert!(matches!(
        pollster::block_on(herald_flows::housekeep_announcing(
            (&config, &connected, &ports),
            &keys_in(&keyring),
            "r1",
        )),
        Err(Stop::Store(_))
    ));
}

#[test]
fn one_tag_a_ci_run_names_is_held_where_its_source_holds_tags_and_announced_otherwise() {
    let tag = {
        let mut attributes = herald_core::versioned("v1", false);
        attributes.insert(
            herald_core::SIGNAL.to_owned(),
            herald_core::Attribute::Text("tag".to_owned()),
        );
        herald_core::Event::normalise(herald_core::Draft {
            identity: "github.com/1/v1".to_owned(),
            kind: herald_core::Kind::RELEASE,
            project: "php-mutation-gate".to_owned(),
            title: "v1".to_owned(),
            body: String::new(),
            links: std::collections::BTreeMap::new(),
            attributes,
            occurred: NOON,
        })
        .unwrap()
    };
    let told = |config: &Config, store: &Failing, script: &Script| {
        let prepared =
            pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
        let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
        let clock = FakeClock::at(NOON);
        let ports = Ports {
            store,
            clock: &clock,
            signer: &FakeSigner,
            budget: Budget::UNBOUNDED,
        };
        pollster::block_on(herald_flows::held_or_announced(
            config,
            &connected,
            &ports,
            (&SourceName::new("gate"), &tag),
            "r1",
        ))
    };
    let store = healthy();
    let held = told(&tagged(None), &store, &Script::default()).unwrap();
    assert_eq!(held.held, Some(NOON + SignedDuration::from_hours(1)));
    assert!(
        pollster::block_on(store.inner.pending(&hold()))
            .unwrap()
            .is_some()
    );
    let script = Script::answering(&[POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]);
    let first = told(
        &tagged(Some(herald_core::Prefer::First)),
        &healthy(),
        &script,
    )
    .unwrap();
    assert_eq!(first.held, None);
    assert_eq!(first.announced.len(), 1);
    let unwritable = Failing {
        fault: Fault::WritesPending,
        ..healthy()
    };
    assert!(matches!(
        told(&tagged(None), &unwritable, &Script::default()),
        Err(Stop::Store(_))
    ));
}

#[test]
fn an_event_naming_no_signal_is_never_held() {
    let untagged = herald_core::Event::normalise(herald_core::Draft {
        identity: "github.com/1/v1".to_owned(),
        kind: herald_core::Kind::RELEASE,
        project: "php-mutation-gate".to_owned(),
        title: "v1".to_owned(),
        body: String::new(),
        links: std::collections::BTreeMap::new(),
        attributes: herald_core::versioned("v1", false),
        occurred: NOON,
    })
    .unwrap();
    let config = tagged(None);
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let script = Script::answering(&[POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]);
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let store = healthy();
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let told = pollster::block_on(herald_flows::held_or_announced(
        &config,
        &connected,
        &ports,
        (&SourceName::new("gate"), &untagged),
        "r1",
    ))
    .unwrap();
    assert_eq!((told.held, told.announced.len()), (None, 1));
    assert_eq!(
        pollster::block_on(store.inner.pending(&hold())).unwrap(),
        None
    );
}
