//! A living post of a view that reads advisories, deleted and posted anew
//! where its destination keeps what an edit replaced, never edited; and
//! deleted where its room's audience no longer passes the check
//! (ADR-0067).

use std::collections::BTreeMap;

use herald_config::{Config, Uses, load};
use herald_flows::{Budget, Connected, Ports, Requests, Swept, prepare, report};
use herald_ports::Response;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use jiff::{SignedDuration, Timestamp};

use crate::advisories::{CODE, DEPENDABOT, GRAPHQL, SECRETS, alerts, page};
use crate::harness::{Failing, Fault, NOON, Script, healthy};

pub(crate) const MARKER: &str = "io.nightworks.herald.marker";

pub(crate) const CLIENT: &str =
    "https://matrix.example/_matrix/client/v3/rooms/%21r%3Amatrix.example";

pub(crate) fn config(private: bool) -> Config {
    configured(private, r#"["advisories"]"#, "")
}

/// The config whose view shows `overviews`, its board posting to one room
/// as `private` says, and `boards` more.
pub(crate) fn configured(private: bool, overviews: &str, boards: &str) -> Config {
    named("live", private, (overviews, boards))
}

/// The config whose view shows `overviews`, its board `board` posting to
/// one room as `private` says, and `boards` more.
pub(crate) fn named(board: &str, private: bool, (overviews, boards): (&str, &str)) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.room]
kind = "matrix"
homeserver = "https://matrix.example"
token = {{ secret = "HERALD_MATRIX_TOKEN" }}
[sources.open]
kind = "github"
repository = "o/open"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[views.security]
sources = ["open"]
overviews = {overviews}
[boards.{board}]
view = "security"
every = "1h"
at = "00:00"
destination = "room"
room = "!r:matrix.example"
template = "herald:queue"
pin = false
private = {private}
{boards}"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

pub(crate) fn answer(body: &str) -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    }
}

/// The forge and the room as one round reads them, the room joined by
/// `join_rule` and Dependabot holding an alert where `open` says, each
/// event herald sends answered with the next of `sent`.
pub(crate) fn world(join_rule: &str, open: bool, sent: &[&str]) -> Script {
    held(
        &[(CLIENT, join_rule)],
        (&alerts(open), BTreeMap::new()),
        sent,
    )
}

/// The forge and each of `rooms` as one round reads them, each room joined
/// by its rule and Dependabot answering `alerts`, `more` answered too; each
/// event herald sends answered with the next of `sent`.
pub(crate) fn held(
    rooms: &[(&str, &str)],
    (alerts, more): (&str, BTreeMap<String, Response>),
    sent: &[&str],
) -> Script {
    let mut keyed = more;
    let page = page();
    keyed
        .entry(GRAPHQL.to_owned())
        .or_insert_with(|| answer(&page));
    for (path, body) in [(CODE, "[]"), (DEPENDABOT, alerts), (SECRETS, "[]")] {
        keyed.insert(format!("https://api.github.com{path}"), answer(body));
    }
    for (client, join_rule) in rooms {
        let rules = format!(r#"{{"join_rule":"{join_rule}"}}"#);
        keyed.insert(format!("{client}/state/m.room.join_rules/"), answer(&rules));
        keyed.insert(
            format!("{client}/state/m.room.history_visibility/"),
            answer(r#"{"history_visibility":"joined"}"#),
        );
    }
    let events: Vec<String> = sent
        .iter()
        .map(|event| format!(r#"{{"event_id":"{event}"}}"#))
        .collect();
    let answers: Vec<(&str, u16, &str)> =
        events.iter().map(|body| ("", 200, body.as_str())).collect();
    Script::keyed(keyed).then(&answers)
}

pub(crate) fn round(
    config: &Config,
    store: &Failing,
    script: &Script,
    at: Timestamp,
) -> Vec<Swept> {
    bounded(config, store, script, (at, None))
}

/// One report round at `at`, making at most `most` requests where it
/// says.
pub(crate) fn bounded(
    config: &Config,
    store: &Failing,
    script: &Script,
    (at, most): (Timestamp, Option<usize>),
) -> Vec<Swept> {
    ran(config, (store, script), (at, most), "r1")
}

/// One report round at `at` by the run `run`, making at most `most`
/// requests where it says.
fn ran(
    config: &Config,
    (store, script): (&Failing, &Script),
    (at, most): (Timestamp, Option<usize>),
    run: &str,
) -> Vec<Swept> {
    let secrets = FakeSecrets::default()
        .with("HERALD_MATRIX_TOKEN", "t0ken")
        .with("HERALD_GITHUB_TOKEN", "ghp_x");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(at);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget {
            requests: most.map(|most| Requests {
                made: script.made(),
                from: script.made().load(std::sync::atomic::Ordering::Relaxed),
                most,
                spare: 0,
            }),
            ..Budget::UNBOUNDED
        },
    };
    pollster::block_on(report(config, &connected, &ports, run))
}

/// The transaction each event `script` sent to the room was sent under.
pub(crate) fn transactions(script: &Script) -> Vec<String> {
    written(script)
        .iter()
        .filter_map(|line| line.split("/send/m.room.message/").nth(1))
        .map(|rest| rest.split_whitespace().next().unwrap_or(rest).to_owned())
        .collect()
}

/// The requests of `script` that wrote to the room.
pub(crate) fn written(script: &Script) -> Vec<String> {
    script
        .seen()
        .into_iter()
        .filter(|line| line.starts_with("PUT "))
        .collect()
}

#[test]
fn a_living_post_with_advisories_is_redacted_and_posted_anew_never_edited_on_matrix() {
    let store = healthy();
    let first = world("invite", true, &["$one"]);
    round(&config(true), &store, &first, NOON);
    assert_eq!(written(&first).len(), 1, "{:?}", first.seen());
    let later = NOON + SignedDuration::from_hours(1);
    let second = world("invite", false, &["$gone", "$two"]);
    round(&config(true), &store, &second, later);
    let writes = written(&second);
    assert_eq!(writes.len(), 2, "{:?}", second.seen());
    assert!(
        writes[0].starts_with(&format!("PUT {CLIENT}/redact/%24one/")),
        "{writes:?}"
    );
    assert!(
        writes[1].starts_with(&format!("PUT {CLIENT}/send/m.room.message/")),
        "{writes:?}"
    );
    let bodies: Vec<String> = second
        .bodies()
        .into_iter()
        .map(|body| String::from_utf8(body).unwrap())
        .collect();
    assert!(
        bodies.iter().all(|body| !body.contains("m.replace")),
        "{bodies:?}"
    );
    let (posted, reposted) = (transactions(&first), transactions(&second));
    assert_eq!((posted.len(), reposted.len()), (1, 1));
    assert_ne!(posted, reposted);
    let back = world("invite", true, &["$gone", "$three"]);
    round(
        &config(true),
        &store,
        &back,
        later + SignedDuration::from_hours(1),
    );
    let again = transactions(&back);
    assert_eq!(again.len(), 1, "{:?}", back.seen());
    assert!(!posted.contains(&again[0]) && !reposted.contains(&again[0]));
}

#[test]
fn a_living_post_whose_room_anyone_may_now_join_is_redacted_before_any_report() {
    let store = healthy();
    let first = world("invite", true, &["$one"]);
    round(&config(true), &store, &first, NOON);
    let later = NOON + SignedDuration::from_hours(1);
    let opened = world("public", true, &["$gone", "$line", "$withheld"]);
    round(&config(true), &store, &opened, later);
    let writes = written(&opened);
    assert!(
        writes
            .first()
            .is_some_and(|first| first.starts_with(&format!("PUT {CLIENT}/redact/%24one/"))),
        "{:?}",
        opened.seen()
    );
    let bodies: Vec<String> = opened
        .bodies()
        .into_iter()
        .map(|body| String::from_utf8(body).unwrap())
        .collect();
    assert!(
        bodies
            .iter()
            .all(|body| !body.contains("Prototype pollution")),
        "{bodies:?}"
    );
    assert!(
        bodies
            .iter()
            .any(|body| body.contains("removed because this route no longer receives it")),
        "{bodies:?}"
    );
    let posted = transactions(&first);
    let recalled = transactions(&opened);
    assert!(!recalled.is_empty(), "{:?}", opened.seen());
    assert!(
        recalled.iter().all(|sent| !posted.contains(sent)),
        "{recalled:?}"
    );
}

#[test]
fn a_reposted_post_that_landed_unrecorded_is_read_back_never_posted_twice() {
    let store = Failing {
        fault: Fault::LandsReplacedPostOnce,
        ..healthy()
    };
    let first = world("invite", true, &["$one"]);
    round(&config(true), &store, &first, NOON);
    let later = NOON + SignedDuration::from_hours(1);
    let cut = world("invite", false, &["$gone", "$two"]);
    round(&config(true), &store, &cut, later);
    let sent: Vec<String> = cut
        .bodies()
        .into_iter()
        .map(|body| String::from_utf8(body).unwrap())
        .filter(|body| body.contains(MARKER))
        .collect();
    assert_eq!(sent.len(), 1, "{:?}", cut.seen());
    let mut keyed = BTreeMap::new();
    keyed.insert(
        "https://matrix.example/_matrix/client/v3/account/whoami".to_owned(),
        answer(r#"{"user_id":"@herald:matrix.example"}"#),
    );
    let event = format!(
        r#"{{"chunk":[{{"type":"m.room.message","sender":"@herald:matrix.example","event_id":"$two","content":{}}}]}}"#,
        sent[0]
    );
    keyed.insert(format!("{CLIENT}/messages?dir=b&limit=100"), answer(&event));
    let resumed = held(&[(CLIENT, "invite")], (&alerts(false), keyed), &[]);
    round(&config(true), &store, &resumed, later);
    assert_eq!(
        written(&resumed),
        Vec::<String>::new(),
        "{:?}",
        resumed.seen()
    );
    assert!(
        resumed
            .seen()
            .iter()
            .any(|line| line.contains("/messages?dir=b")),
        "{:?}",
        resumed.seen()
    );
}

#[test]
fn a_section_whose_view_no_longer_reads_advisories_is_recalled_before_the_next_post() {
    let store = healthy();
    let first = world("invite", true, &["$one"]);
    round(&config(true), &store, &first, NOON);
    let later = NOON + SignedDuration::from_hours(1);
    let dropped = world("invite", true, &["$gone", "$line", "$board"]);
    round(&configured(true, "[]", ""), &store, &dropped, later);
    let writes = written(&dropped);
    assert!(
        writes
            .first()
            .is_some_and(|first| first.starts_with(&format!("PUT {CLIENT}/redact/%24one/"))),
        "{writes:?}"
    );
    let bodies: Vec<String> = dropped
        .bodies()
        .into_iter()
        .map(|body| String::from_utf8(body).unwrap())
        .collect();
    assert!(
        bodies
            .iter()
            .any(|body| body.contains("removed because this route no longer receives it")),
        "{bodies:?}"
    );
    assert!(
        bodies
            .iter()
            .all(|body| !body.contains("Prototype pollution")),
        "{bodies:?}"
    );
}

#[test]
fn a_replacement_that_cannot_read_back_or_record_its_start_sends_nothing() {
    let store = Failing {
        fault: Fault::LandsReplacedPostOnce,
        ..healthy()
    };
    let first = world("invite", true, &["$one"]);
    round(&config(true), &store, &first, NOON);
    let later = NOON + SignedDuration::from_hours(1);
    let cut = world("invite", false, &["$gone", "$two"]);
    round(&config(true), &store, &cut, later);
    let unread = world("invite", true, &[]);
    round(
        &config(true),
        &store,
        &unread,
        later + SignedDuration::from_hours(1),
    );
    assert_eq!(
        written(&unread),
        Vec::<String>::new(),
        "{:?}",
        unread.seen()
    );
    let mut fresh = healthy();
    round(
        &config(true),
        &fresh,
        &world("invite", true, &["$one"]),
        NOON,
    );
    *fresh.writes.lock().unwrap() = 0;
    fresh.fault = Fault::Claims;
    let unclaimed = world("invite", false, &["$gone", "$two"]);
    round(&config(true), &fresh, &unclaimed, later);
    assert_eq!(
        written(&unclaimed),
        Vec::<String>::new(),
        "{:?}",
        unclaimed.seen()
    );
}

#[test]
fn a_replacement_another_run_holds_is_left_to_it() {
    let mut store = healthy();
    round(
        &config(true),
        &store,
        &world("invite", true, &["$one"]),
        NOON,
    );
    let later = NOON + SignedDuration::from_hours(1);
    store.fault = Fault::Releases;
    round(
        &config(true),
        &store,
        &world("invite", false, &["$gone", "$two"]),
        later,
    );
    store.fault = Fault::Nothing;
    let other = world("invite", false, &[]);
    let swept = ran(
        &config(true),
        (&store, &other),
        (later + SignedDuration::from_mins(1), None),
        "r2",
    );
    assert!(format!("{swept:?}").contains("Held {"), "{swept:?}");
    assert_eq!(written(&other), Vec::<String>::new(), "{:?}", other.seen());
}
