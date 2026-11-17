use std::collections::BTreeMap;

use herald_core::{
    Draft, Event, HeldEvent, Hold, Kind, Listed, Membership, Pending, Reach, Visibility,
};
use herald_flows::{Outcome, Within, announce_within};
use herald_ports::Waiting;

use super::*;

/// A private place beside the harness's public route.
const TEAM: &str = r#"
[[routes]]
id = "team"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000300"
template = "release"
private = true
"#;

fn tag() -> Event {
    Event::normalise(Draft {
        identity: "github.com/1/v1".to_owned(),
        kind: Kind::RELEASE,
        project: "herald".to_owned(),
        title: "v1".to_owned(),
        body: String::new(),
        links: BTreeMap::new(),
        attributes: herald_core::versioned("v1", false),
        occurred: NOON,
    })
    .unwrap()
}

/// The member `herald` of `gate`, kept as `visibility`.
fn kept(store: &Failing, visibility: Visibility) {
    let membership = Membership {
        repository: Listed {
            id: "1".to_owned(),
            name: "herald".to_owned(),
            path: "nightworksio/herald".to_owned(),
            topics: Vec::new(),
            properties: BTreeMap::new(),
            visibility,
            archived: false,
            fork: false,
            template: false,
            language: None,
            linked: None,
        },
        discovered: NOON,
        left: None,
        cursor: None,
        collides: None,
    };
    let held = pollster::block_on(store.inner.member(&SourceName::new("gate"), "1")).unwrap();
    pollster::block_on(store.inner.put_member(
        &SourceName::new("gate"),
        &membership,
        held.map(|held| held.version),
    ))
    .unwrap();
}

/// The tag held for the member `herald`, due at noon.
fn held(store: &Failing) {
    let pending = Pending {
        hold: Hold::Tag {
            source: SourceName::new("gate"),
            identity: tag().identity().clone(),
        },
        events: vec![HeldEvent {
            source: SourceName::new("gate"),
            member: Some("1".to_owned()),
            event: tag(),
        }],
        opened: NOON,
        due: NOON,
    };
    pollster::block_on(store.inner.put_pending(&pending, None)).unwrap();
}

/// The routes each released event of `swept` went to.
fn released(swept: &[(Option<String>, Sweep)]) -> Vec<Vec<String>> {
    swept
        .iter()
        .filter_map(|(_, sweep)| match sweep {
            Sweep::Released { announced, .. } => Some(
                announced
                    .iter()
                    .flat_map(|fresh| {
                        fresh
                            .announced
                            .iter()
                            .map(|done| done.route.as_str().to_owned())
                    })
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn a_held_tag_of_a_member_turned_private_reaches_only_the_private_routes() {
    let store = healthy();
    let config = owned_in(TEAM, |options| {
        options.signals = Some(vec![herald_core::Signal::Release, herald_core::Signal::Tag]);
        options.visibility = Some(vec![Visibility::Public, Visibility::Private]);
    });
    kept(&store, Visibility::Private);
    held(&store);
    let private = repositories(&[(1, "herald")]).replace("public", "private");
    let posted = (
        "/channels/1566200000000000300/messages",
        200,
        r#"{"id":"3"}"#,
    );
    let answers = [
        posted,
        (LISTING, 200, private.as_str()),
        ("/repos/nightworksio/herald/releases?per_page=30", 304, ""),
        ("/repos/nightworksio/herald/tags?per_page=30", 304, ""),
    ];
    let swept = polled(&config, &store, &Script::answering(&answers), NOON);
    assert_eq!(released(&swept), [vec!["team".to_owned()]]);
}

/// What the held events due at noon were released as, each request
/// answered by `script`.
fn swept_held(config: &Config, store: &Failing, script: &Script) -> Vec<Swept> {
    let prepared =
        pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = herald_flows::Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    pollster::block_on(herald_flows::announce_held(
        config, &connected, &ports, "r1",
    ))
    .unwrap()
}

#[test]
fn a_held_tag_of_a_member_is_bounded_by_its_record_whether_or_not_its_source_still_expands() {
    let store = healthy();
    kept(&store, Visibility::Private);
    held(&store);
    let swept = swept_held(&config(""), &store, &Script::default());
    let Sweep::Released { announced, .. } = &swept[0].outcome else {
        panic!("{swept:?}");
    };
    assert_eq!(announced[0].event.visibility(), Some(Visibility::Private));
    assert_eq!(announced[0].announced, Vec::new());
    let store = healthy();
    kept(&store, Visibility::Private);
    held(&store);
    let posted = (
        "/channels/1566200000000000300/messages",
        200,
        r#"{"id":"3"}"#,
    );
    let swept = swept_held(
        &owned_in(TEAM, |_| ()),
        &store,
        &Script::answering(&[posted]),
    );
    let Sweep::Released { announced, .. } = &swept[0].outcome else {
        panic!("{swept:?}");
    };
    let origin = announced[0].event.origin().unwrap();
    assert_eq!(origin.repository, "nightworksio/herald");
    assert_eq!(origin.visibility, Visibility::Private);
    let routes: Vec<&str> = announced[0]
        .announced
        .iter()
        .map(|done| done.route.as_str())
        .collect();
    assert_eq!(routes, ["team"]);
}

#[test]
fn a_member_s_event_saying_nothing_of_who_may_see_it_reaches_only_the_private_routes() {
    let store = healthy();
    let config = config(TEAM);
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let posted = (
        "/channels/1566200000000000300/messages",
        200,
        r#"{"id":"3"}"#,
    );
    let script = Script::answering(&[posted]);
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = herald_flows::Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let reach = Reach::default();
    let announced = pollster::block_on(announce_within(
        &config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        &tag(),
        Within {
            routes: None,
            reach: Some(&reach),
        },
        "r1",
    ));
    let routes: Vec<&str> = announced.iter().map(|done| done.route.as_str()).collect();
    assert_eq!(routes, ["team"]);
    assert!(matches!(announced[0].outcome, Outcome::Sent { .. }));
}
