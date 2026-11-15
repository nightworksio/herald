use jiff::SignedDuration;
use serde_json::json;

use super::*;

#[test]
fn a_first_listing_s_members_remember_their_tags_unread_and_a_member_that_joins_reads_them() {
    let store = healthy();
    let config = owned(|options| options.signals = Some(vec![herald_core::Signal::Tag]));
    let page = |id: u64, name: &str| {
        json!({"id": id, "name": name, "html_url": format!("https://github.com/nightworksio/{name}")})
            .to_string()
    };
    let (herald_page, gate_page) = (page(1, "herald"), page(2, "gate"));
    let first_round = Script::answering(&[
        (LISTING, 200, &repositories(&[(1, "herald")])),
        (
            "/repos/nightworksio/herald/tags?per_page=30",
            200,
            r#"[{"name":"v1"}]"#,
        ),
        ("/repos/nightworksio/herald", 200, &herald_page),
    ]);
    assert_eq!(
        polled(&config, &store, &first_round, NOON),
        [listed(1, &["herald"], &[]), member("herald", first(0))]
    );
    let tagged_before = json!({"committer": {"date": NOON.to_string()}}).to_string();
    let joined = NOON + SignedDuration::from_hours(1);
    let second_round = Script::answering(&[
        (LISTING, 200, &repositories(&[(1, "herald"), (2, "gate")])),
        (
            "/repos/nightworksio/herald/tags?per_page=30",
            200,
            r#"[{"name":"v1"}]"#,
        ),
        ("/repos/nightworksio/herald", 200, &herald_page),
        (
            "/repos/nightworksio/gate/tags?per_page=30",
            200,
            r#"[{"name":"v2"}]"#,
        ),
        ("/repos/nightworksio/gate", 200, &gate_page),
        (
            "/repos/nightworksio/gate/git/ref/tags/v2",
            200,
            r#"{"object":{"type":"commit","sha":"c2"}}"#,
        ),
        (
            "/repos/nightworksio/gate/git/commits/c2",
            200,
            &tagged_before,
        ),
    ]);
    assert_eq!(
        polled(&config, &store, &second_round, joined),
        [
            listed(2, &["gate"], &[]),
            member(
                "herald",
                Sweep::Polled {
                    first: false,
                    found: 0,
                    announced: Vec::new(),
                    cursor: Remembered::Advanced,
                }
            ),
            member("gate", first(1)),
        ]
    );
    assert!(second_round.answers.lock().unwrap().is_empty());
}

#[test]
fn a_held_tag_of_a_member_still_listed_is_announced_as_the_member_s() {
    use herald_ports::Waiting;
    let store = healthy();
    let config = owned(|options| {
        options.signals = Some(vec![herald_core::Signal::Release, herald_core::Signal::Tag]);
    });
    let page =
        json!({"id": 1, "name": "herald", "html_url": "https://github.com/nightworksio/herald"})
            .to_string();
    let listing = repositories(&[(1, "herald")]);
    let quiet = || {
        vec![
            (LISTING, 200, listing.as_str()),
            ("/repos/nightworksio/herald/releases?per_page=30", 200, "[]"),
            ("/repos/nightworksio/herald/tags?per_page=30", 200, "[]"),
            ("/repos/nightworksio/herald", 200, page.as_str()),
        ]
    };
    polled(&config, &store, &Script::answering(&quiet()), NOON);
    let event = herald_core::Event::normalise(herald_core::Draft {
        identity: "github.com/1/v1".to_owned(),
        kind: herald_core::Kind::RELEASE,
        project: "herald".to_owned(),
        title: "v1".to_owned(),
        body: String::new(),
        links: std::collections::BTreeMap::new(),
        attributes: herald_core::versioned("v1", false),
        occurred: NOON,
    })
    .unwrap();
    let hold = herald_core::Hold::Tag {
        source: SourceName::new("gate"),
        identity: event.identity().clone(),
    };
    let pending = herald_core::Pending {
        hold: hold.clone(),
        events: vec![herald_core::HeldEvent {
            source: SourceName::new("gate"),
            member: Some("1".to_owned()),
            event,
        }],
        opened: NOON,
        due: NOON,
    };
    pollster::block_on(store.inner.put_pending(&pending, None)).unwrap();
    let mut answers = vec![POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS];
    answers.extend(quiet());
    let swept = polled(
        &config,
        &store,
        &Script::answering(&answers),
        NOON + SignedDuration::from_hours(1),
    );
    let (
        member,
        Sweep::Released {
            announced,
            withheld,
        },
    ) = &swept[0]
    else {
        panic!("{swept:?}");
    };
    assert_eq!(member.as_deref(), Some("nightworksio/herald"));
    assert_eq!(withheld, &None);
    assert_eq!(announced.len(), 1);
    assert!(matches!(
        announced[0].announced[0].outcome,
        herald_flows::Outcome::Sent { .. }
    ));
    assert_eq!(
        pollster::block_on(store.inner.pending(&hold)).unwrap(),
        None
    );
}

#[test]
fn held_events_that_cannot_be_read_stop_an_owner_holding_tags_whose_members_are_still_polled() {
    let store = Failing {
        fault: Fault::ReadsPending,
        ..healthy()
    };
    let config = owned(|options| {
        options.signals = Some(vec![herald_core::Signal::Release, herald_core::Signal::Tag]);
    });
    let page =
        json!({"id": 1, "name": "herald", "html_url": "https://github.com/nightworksio/herald"})
            .to_string();
    let listing = repositories(&[(1, "herald")]);
    let script = Script::answering(&[
        (LISTING, 200, &listing),
        ("/repos/nightworksio/herald/releases?per_page=30", 200, "[]"),
        ("/repos/nightworksio/herald/tags?per_page=30", 200, "[]"),
        ("/repos/nightworksio/herald", 200, &page),
    ]);
    let swept = polled(&config, &store, &script, NOON);
    assert!(
        matches!(swept[0], (None, Sweep::Stopped(Stop::Store(_)))),
        "{swept:?}"
    );
    assert_eq!(
        swept[1..],
        [listed(1, &["herald"], &[]), member("herald", first(0))]
    );
}
