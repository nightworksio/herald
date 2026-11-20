//! A post or chunk herald sent for a living post shown advisories and never
//! recorded: read back and deleted when a replacement begins, never taken
//! for the new post; and kept, and named by doctor, where it cannot be
//! found again (ADR-0067).

use std::collections::BTreeMap;

use herald_core::media::JSON;
use herald_core::{
    AdvisedPost, Cursor, DestinationName, Entry, Identity, Key, RouteId, SourceName, Step, Stray,
    living_identity,
};
use herald_flows::{orphans, orphans_cleared, recalls};
use herald_ports::{Cursors, Ledger, Response, Store};
use jiff::SignedDuration;

use crate::advisories::alerts;
use crate::harness::{Failing, Fault, NOON, Script, healthy};
use crate::replaced::{CLIENT, MARKER, answer, config, held, round, transactions, world, written};
use crate::sealed::listed;

/// The room's answers to herald reading its own posts back: `events`,
/// newest first, and no pin.
fn read_back(events: &[String]) -> BTreeMap<String, Response> {
    let mut keyed = BTreeMap::new();
    keyed.insert(
        "https://matrix.example/_matrix/client/v3/account/whoami".to_owned(),
        answer(r#"{"user_id":"@herald:matrix.example"}"#),
    );
    keyed.insert(
        format!("{CLIENT}/messages?dir=b&limit=100"),
        answer(&format!(r#"{{"chunk":[{}]}}"#, events.join(","))),
    );
    keyed.insert(
        format!("{CLIENT}/state/m.room.pinned_events"),
        answer(r#"{"pinned":[]}"#),
    );
    keyed
}

/// One event herald sent to the room, as the room lists it.
fn event(id: &str, content: &str) -> String {
    format!(
        r#"{{"type":"m.room.message","sender":"@herald:matrix.example","event_id":"{id}","content":{content}}}"#
    )
}

/// The bodies `script` sent that carry herald's marker.
fn marked(script: &Script) -> Vec<String> {
    script
        .bodies()
        .into_iter()
        .map(|body| String::from_utf8(body).unwrap())
        .filter(|body| body.contains(MARKER))
        .collect()
}

/// A replacement's post sent for a new section, landed and never recorded.
fn unrecorded(store: &Failing) -> String {
    round(
        &config(true),
        store,
        &world("invite", true, &["$one"]),
        NOON,
    );
    let cut = world("invite", false, &["$gone", "$two"]);
    round(
        &config(true),
        store,
        &cut,
        NOON + SignedDuration::from_hours(1),
    );
    let sent = marked(&cut);
    assert_eq!(sent.len(), 1, "{:?}", cut.seen());
    sent[0].clone()
}

/// What doctor names as possibly orphaned of the report `live`.
fn orphaned(store: &Failing) -> Vec<String> {
    let found = pollster::block_on(recalls(
        &config(true),
        (&store.cursors(), &store.recalls(), &store.ledger()),
    ));
    found
        .mistakes
        .into_iter()
        .filter(|mistake| mistake.contains("orphaned"))
        .collect()
}

fn redacted(script: &Script, event: &str) -> bool {
    written(script)
        .iter()
        .any(|write| write.starts_with(&format!("PUT {CLIENT}/redact/{event}/")))
}

#[test]
fn a_recall_deletes_the_post_a_replacement_sent_and_never_recorded() {
    let store = Failing {
        fault: Fault::LandsReplacedPostOnce,
        ..healthy()
    };
    let sent = unrecorded(&store);
    let found = read_back(&[event("$two", &sent)]);
    let opened = held(
        &[(CLIENT, "public")],
        (&alerts(false), found),
        &["$redaction", "$line", "$board"],
    );
    round(
        &config(true),
        &store,
        &opened,
        NOON + SignedDuration::from_hours(2),
    );
    assert!(redacted(&opened, "%24two"), "{:?}", opened.seen());
    assert!(
        marked(&opened)
            .iter()
            .any(|body| body.contains("removed because this route no longer receives it")),
        "{:?}",
        opened.seen()
    );
}

#[test]
fn a_recall_deletes_a_chunk_sent_and_never_recorded() {
    let store = Failing {
        fault: Fault::LandsChunkOnce,
        ..healthy()
    };
    let first = held(
        &[(CLIENT, "invite")],
        (&listed(400), BTreeMap::new()),
        &["$one", "$chunk", "$more"],
    );
    round(&config(true), &store, &first, NOON);
    let sent = marked(&first);
    assert_eq!(written(&first).len(), 2, "{:?}", first.seen());
    let reply = r#"{"m.relates_to":{"rel_type":"m.thread","event_id":"$one"}}"#;
    let found = read_back(&[event("$chunk", reply), event("$one", &sent[0])]);
    let opened = held(
        &[(CLIENT, "public")],
        (&alerts(false), found),
        &["$redaction", "$redaction", "$redaction", "$line", "$board"],
    );
    round(
        &config(true),
        &store,
        &opened,
        NOON + SignedDuration::from_hours(1),
    );
    assert!(redacted(&opened, "%24one"), "{:?}", opened.seen());
    assert!(redacted(&opened, "%24chunk"), "{:?}", opened.seen());
}

#[test]
fn a_post_never_found_again_is_kept_and_named_until_the_operator_clears_it() {
    let store = Failing {
        fault: Fault::LandsReplacedPostOnce,
        ..healthy()
    };
    unrecorded(&store);
    let opened = held(
        &[(CLIENT, "public")],
        (&alerts(false), read_back(&[])),
        &["$line", "$redaction", "$board"],
    );
    round(
        &config(true),
        &store,
        &opened,
        NOON + SignedDuration::from_hours(2),
    );
    let advised = |store: &Failing| {
        pollster::block_on(store.cursors().cursor(&SourceName::new("live")))
            .unwrap()
            .map_or(0, |cursor| cursor.value.advised.len())
    };
    assert_eq!(advised(&store), 1, "{:?}", opened.seen());
    let named = orphaned(&store);
    assert!(
        named.len() == 1
            && named[0].contains(
                "then run `herald orphans clear live --route live` and the command it prints"
            ),
        "{named:?}"
    );
    let live = RouteId::new("live");
    let listed = pollster::block_on(orphans(&store, "live", None)).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!((&listed[0].route, &listed[0].parts), (&live, &vec![0]));
    let seen = listed[0].seen.clone();
    assert_eq!(orphaned(&store).len(), 1);
    let elsewhere = RouteId::new("lobby");
    let other = pollster::block_on(orphans_cleared(&store, "live", (&elsewhere, &seen)));
    assert!(
        other.is_err_and(|refused| refused.contains("on the route lobby")),
        "the route filter cleared another route's parts"
    );
    assert_eq!(orphaned(&store).len(), 1);
    let dropped = pollster::block_on(orphans_cleared(&store, "live", (&live, &seen))).unwrap();
    assert_eq!(dropped, listed[0]);
    assert_eq!(orphaned(&store), Vec::<String>::new());
    let refused = pollster::block_on(orphans_cleared(&store, "live", (&live, &seen))).unwrap_err();
    assert!(refused.contains("nothing was cleared"), "{refused}");
    let none = pollster::block_on(orphans(&store, "live", None)).unwrap_err();
    assert!(none.contains("no living post of the report live"), "{none}");
    let next = held(
        &[(CLIENT, "public")],
        (&alerts(false), read_back(&[])),
        &["$redaction", "$line", "$board"],
    );
    round(
        &config(true),
        &store,
        &next,
        NOON + SignedDuration::from_hours(3),
    );
    assert_eq!(advised(&store), 0, "{:?}", next.seen());
}

#[test]
fn a_resumed_replacement_sends_its_post_again_under_its_first_key_and_leaves_no_stray() {
    let store = Failing {
        fault: Fault::LandsReplacedPostOnce,
        ..healthy()
    };
    let cut = world("invite", true, &["$one"]);
    round(&config(true), &store, &cut, NOON);
    let later = NOON + SignedDuration::from_hours(1);
    let lost = world("invite", false, &["$gone", "$two"]);
    round(&config(true), &store, &lost, later);
    let resumed = held(
        &[(CLIENT, "invite")],
        (&alerts(false), read_back(&[])),
        &["$two"],
    );
    round(
        &config(true),
        &store,
        &resumed,
        later + SignedDuration::from_mins(10),
    );
    assert_eq!(
        transactions(&resumed),
        transactions(&lost),
        "{:?}",
        resumed.seen()
    );
    assert_eq!(orphaned(&store), Vec::<String>::new());
}

#[test]
fn a_chunk_sent_again_after_its_answer_was_lost_goes_under_its_first_key() {
    let store = Failing {
        fault: Fault::LandsChunkOnce,
        ..healthy()
    };
    let first = held(
        &[(CLIENT, "invite")],
        (&listed(400), BTreeMap::new()),
        &["$one", "$chunk"],
    );
    round(&config(true), &store, &first, NOON);
    let again = held(
        &[(CLIENT, "invite")],
        (&listed(400), read_back(&[])),
        &["$chunk"],
    );
    round(
        &config(true),
        &store,
        &again,
        NOON + SignedDuration::from_mins(10),
    );
    let (sent, resent) = (transactions(&first), transactions(&again));
    assert_eq!(resent.first(), sent.get(1), "{:?}", again.seen());
    assert_eq!(orphaned(&store), Vec::<String>::new());
}

#[test]
fn a_report_gone_from_the_config_still_has_its_unrecorded_post_deleted() {
    let store = Failing {
        fault: Fault::LandsReplacedPostOnce,
        ..healthy()
    };
    let sent = unrecorded(&store);
    let gone = held(
        &[(CLIENT, "invite")],
        (&alerts(false), read_back(&[event("$two", &sent)])),
        &["$redaction", "$line", "$other"],
    );
    let other = crate::replaced::named("other", true, (r#"["advisories"]"#, ""));
    round(&other, &store, &gone, NOON + SignedDuration::from_hours(2));
    assert!(redacted(&gone, "%24two"), "{:?}", gone.seen());
}

#[test]
fn a_post_the_room_refused_is_never_taken_for_a_stray() {
    let store = healthy();
    let refused = answered(&store, (403, JSON, r#"{"errcode":"M_FORBIDDEN"}"#));
    let opened = held(
        &[(CLIENT, "public")],
        (&alerts(false), read_back(&[])),
        &["$line", "$redaction", "$board"],
    );
    round(
        &config(true),
        &store,
        &opened,
        NOON + SignedDuration::from_hours(2),
    );
    assert!(
        written(&refused)
            .iter()
            .any(|write| write.contains("/send/")),
        "{:?}",
        refused.seen()
    );
    assert_eq!(
        orphaned(&store),
        Vec::<String>::new(),
        "{:?}",
        opened.seen()
    );
}

/// The ledger entry of the report `live`'s living post.
fn living(store: &Failing) -> herald_core::Entry {
    let key = herald_core::Key::derive(
        &RouteId::new("live"),
        &herald_core::Identity::new(herald_core::living_identity("live")),
    );
    pollster::block_on(Store::ledger(store).entry(&key))
        .unwrap()
        .unwrap()
        .value
}

/// A round replacing the living post whose post is answered `status`, as
/// the media type `typed`, with `body`, after its earlier post was
/// redacted.
fn answered(store: &Failing, (status, typed, body): (u16, &str, &str)) -> Script {
    round(
        &config(true),
        store,
        &world("invite", true, &["$one"]),
        NOON,
    );
    let cut = held(
        &[(CLIENT, "invite")],
        (&alerts(false), BTreeMap::new()),
        &[],
    )
    .then(&[("", 200, r#"{"event_id":"$gone"}"#), ("", status, body)])
    .headed(&[("Content-Type", typed)]);
    round(
        &config(true),
        store,
        &cut,
        NOON + SignedDuration::from_hours(1),
    );
    cut
}

#[test]
fn a_post_answered_with_a_server_error_or_no_id_is_read_back_never_posted_twice() {
    for answer in [
        (504, JSON, "{}"),
        (200, JSON, "{}"),
        (408, JSON, "{}"),
        (429, JSON, "{}"),
        (
            403,
            "text/html; charset=utf-8",
            "<html><body>Request blocked</body></html>",
        ),
        (403, JSON, r#"{"detail":"blocked by the gateway"}"#),
        (403, "text/plain", r#"{"errcode":"M_FORBIDDEN"}"#),
    ] {
        let store = healthy();
        let cut = answered(&store, answer);
        let sent = marked(&cut);
        assert_eq!(sent.len(), 1, "{answer:?} {:?}", cut.seen());
        let resumed = held(
            &[(CLIENT, "invite")],
            (&alerts(false), read_back(&[event("$two", &sent[0])])),
            &[],
        );
        round(
            &config(true),
            &store,
            &resumed,
            NOON + SignedDuration::from_hours(1),
        );
        assert_eq!(written(&resumed), Vec::<String>::new(), "{answer:?}");
        assert_eq!(living(&store).part(0), Some("$two"), "{answer:?}");
    }
}

#[test]
fn a_first_post_declined_outright_is_dropped_and_a_declined_retry_is_kept() {
    let store = healthy();
    answered(&store, (403, JSON, r#"{"errcode":"M_FORBIDDEN"}"#));
    assert_eq!(living(&store).unrecorded(), []);
    let retried = healthy();
    answered(&retried, (504, JSON, "{}"));
    let declined = held(&[(CLIENT, "invite")], (&alerts(false), read_back(&[])), &[])
        .then(&[("", 403, r#"{"errcode":"M_FORBIDDEN"}"#)])
        .headed(&[("Content-Type", JSON)]);
    round(
        &config(true),
        &retried,
        &declined,
        NOON + SignedDuration::from_hours(1),
    );
    assert_eq!(written(&declined).len(), 1, "{:?}", declined.seen());
    assert_eq!(living(&retried).unrecorded(), [Step::Post]);
}

/// The report `live`'s living posts on the routes `a` and `b`, each with
/// one part sent and never found again.
fn strayed() -> Failing {
    let store = healthy();
    let posts: Vec<AdvisedPost> = ["a", "b"]
        .into_iter()
        .map(|route| AdvisedPost {
            route: RouteId::new(route),
            destination: DestinationName::new("room"),
            placed: String::new(),
        })
        .collect();
    let cursor = Cursor {
        advised: posts.clone(),
        ..Cursor::default()
    };
    pollster::block_on(
        store
            .cursors()
            .put_cursor(&SourceName::new("live"), &cursor, None),
    )
    .unwrap();
    for post in &posts {
        let mut entry = Entry::new(Key::derive(
            &post.route,
            &Identity::new(living_identity("live")),
        ));
        entry.strays.push(Stray {
            part: 0,
            token: format!("{}.first", post.route.as_str()),
        });
        pollster::block_on(Store::ledger(&store).put_entry(&entry, None)).unwrap();
    }
    store
}

/// The parts of the report `live`'s living post on `route` sent and
/// never found again.
fn strays(store: &Failing, route: &str) -> Vec<Stray> {
    let key = Key::derive(
        &RouteId::new(route),
        &Identity::new(living_identity("live")),
    );
    pollster::block_on(Store::ledger(store).entry(&key))
        .unwrap()
        .unwrap()
        .value
        .strays
}

#[test]
fn a_clear_naming_parts_other_than_those_held_now_clears_nothing() {
    let store = strayed();
    let (a, b) = (RouteId::new("a"), RouteId::new("b"));
    let listed = pollster::block_on(orphans(&store, "live", None)).unwrap();
    assert_eq!(listed.len(), 2);
    assert_ne!(listed[0].seen, listed[1].seen);
    let key = Key::derive(&a, &Identity::new(living_identity("live")));
    let entry = pollster::block_on(Store::ledger(&store).entry(&key))
        .unwrap()
        .unwrap();
    let mut later = entry.value;
    later.strays.push(Stray {
        part: 0,
        token: "a.later".to_owned(),
    });
    pollster::block_on(Store::ledger(&store).put_entry(&later, Some(entry.version))).unwrap();
    let changed = pollster::block_on(orphans_cleared(&store, "live", (&a, &listed[0].seen)));
    assert!(
        changed.as_ref().is_err_and(|refused| refused
            .contains("changed since they were listed, so nothing was cleared; list them again with `herald orphans clear live --route a`")),
        "{changed:?}"
    );
    let crossed = pollster::block_on(orphans_cleared(&store, "live", (&a, &listed[1].seen)));
    assert!(crossed.is_err(), "{crossed:?}");
    assert_eq!(strays(&store, "a").len(), 2);
    assert_eq!(strays(&store, "b").len(), 1);
    let again = pollster::block_on(orphans(&store, "live", Some(&a))).unwrap();
    assert_eq!(again[0].parts, [0, 0]);
    assert_ne!(again[0].seen, listed[0].seen);
    let other = pollster::block_on(orphans(&store, "live", Some(&b))).unwrap();
    assert_eq!(other, [listed[1].clone()]);
}

#[test]
fn a_clear_naming_the_parts_listed_clears_that_route_alone() {
    let store = strayed();
    let a = RouteId::new("a");
    let listed = pollster::block_on(orphans(&store, "live", None)).unwrap();
    let routes: Vec<&str> = listed.iter().map(|found| found.route.as_str()).collect();
    assert_eq!(routes, ["a", "b"]);
    let cleared = pollster::block_on(orphans_cleared(&store, "live", (&a, &listed[0].seen)));
    assert_eq!(cleared, Ok(listed[0].clone()));
    assert_eq!(strays(&store, "a"), []);
    assert_eq!(strays(&store, "b").len(), 1);
    let left = pollster::block_on(orphans(&store, "live", None)).unwrap();
    assert_eq!(left, [listed[1].clone()]);
}
