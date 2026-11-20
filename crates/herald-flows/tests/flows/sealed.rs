//! What a run says of, and in what order it sends to, the routes of a view
//! that reads advisories: nothing that tells how many there are or when
//! they changed (ADR-0067).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use herald_ports::Response;
use jiff::SignedDuration;

use crate::advisories::{
    ALERT_PAGE, DEPENDABOT, GRAPHQL, WITHHELD, alerts, page, posted, reported, routed,
};
use crate::harness::{NOON, POSTED, Script, healthy};
use crate::replaced::{CLIENT, answer, bounded, config, configured, held, round, written};

const OPEN: &str = "https://matrix.example/_matrix/client/v3/rooms/%21p%3Amatrix.example";

const OPEN_BOARD: &str = r#"[boards.lobby]
view = "security"
every = "1h"
at = "00:00"
destination = "room"
room = "!p:matrix.example"
template = "herald:queue"
pin = false
private = true
"#;

/// Dependabot's listing holding `count` open alerts.
pub(crate) fn listed(count: u64) -> String {
    let alerts: Vec<String> = (1..=count)
        .map(|number| {
            format!(
                r#"{{"number":{number},"state":"open","html_url":"https://github.com/o/open/security/dependabot/{number}","created_at":"2026-10-01T10:00:00Z","security_advisory":{{"summary":"Prototype pollution {number}","severity":"critical"}}}}"#
            )
        })
        .collect();
    format!("[{}]", alerts.join(","))
}

#[test]
fn what_a_run_says_of_a_route_shown_advisories_is_the_same_however_many_and_whatever_changed() {
    let (none, many) = (healthy(), healthy());
    let sent = &["$one", "$two", "$three", "$four"];
    let room = [(CLIENT, "invite")];
    let first = |store, count| {
        let script = held(&room, (&listed(count), BTreeMap::new()), sent);
        format!("{:?}", round(&config(true), store, &script, NOON))
    };
    assert_eq!(first(&none, 0), first(&many, 40));
    let later = NOON + SignedDuration::from_hours(1);
    let next = |store, count| {
        let script = held(&room, (&listed(count), BTreeMap::new()), sent);
        let said = format!("{:?}", round(&config(true), store, &script, later));
        (said, script.seen().len())
    };
    let (unchanged, quiet) = next(&none, 0);
    let (changed, busy) = next(&many, 1);
    assert!(busy > quiet, "the changed board was not sent anew");
    assert_eq!(unchanged, changed);
}

#[test]
fn every_withheld_line_is_sent_before_any_advisory_is_read_however_many_there_are() {
    let mut said = Vec::new();
    for count in [0, 40] {
        let script = held(
            &[(CLIENT, "invite"), (OPEN, "public")],
            (&listed(count), BTreeMap::new()),
            &["$a", "$b"],
        );
        let config = configured(true, r#"["advisories"]"#, OPEN_BOARD);
        let swept = round(&config, &healthy(), &script, NOON);
        let seen = script.seen();
        let read = seen
            .iter()
            .position(|line| line.contains("/dependabot/alerts"));
        let withheld = seen
            .iter()
            .position(|line| line.starts_with(&format!("PUT {OPEN}/send/")));
        assert!(
            matches!((withheld, read), (Some(withheld), Some(read)) if withheld < read),
            "{seen:?}"
        );
        let lobby = swept.iter().find(|swept| swept.source.as_str() == "lobby");
        said.push(format!("{lobby:?}"));
    }
    assert_eq!(said[0], said[1]);
}

#[test]
fn a_private_route_that_keeps_no_living_post_is_sent_the_line_and_nothing_is_read() {
    let config = crate::advisories::config(&format!(
        "{}{}",
        reported("weekly"),
        routed("weekly", "kept", true).replace("living = true", "living = false")
    ));
    let page = crate::advisories::page();
    let script = Script::answering(&[(crate::advisories::GRAPHQL, 200, &page), POSTED]);
    crate::advisories::round(&config, (&healthy(), &script), (NOON, None));
    let bodies = posted(&script);
    assert_eq!(bodies.len(), 1, "{:?}", script.seen());
    assert!(bodies[0].contains(WITHHELD), "{}", bodies[0]);
    assert!(!bodies[0].contains(ALERT_PAGE), "{}", bodies[0]);
    assert!(
        script.seen().iter().all(|line| !line.contains(DEPENDABOT)),
        "{:?}",
        script.seen()
    );
}

const LOBBY: &str = r#"[boards.lobby]
view = "security"
every = "1h"
at = "00:00"
destination = "room"
room = "!p:matrix.example"
template = "herald:queue"
pin = false
private = false
"#;

/// A second route of the report `live`, to a room no section reaches.
const LOBBY_ROUTE: &str = r#"[[routes]]
id = "lobby"
sources = ["live"]
destination = "room"
room = "!p:matrix.example"
template = "herald:queue"
private = false
living = true
pin = false
"#;

/// The forge's answers with its pull request renamed, so a living post of
/// its rows changes.
fn renamed() -> BTreeMap<String, Response> {
    let page = page().replace("\"Change\"", "\"Renamed\"");
    BTreeMap::from([(GRAPHQL.to_owned(), answer(&page))])
}

/// What `script` wrote to the room no section reaches.
fn lobby(script: &Script) -> Vec<String> {
    written(script)
        .into_iter()
        .filter(|write| write.starts_with(&format!("PUT {OPEN}/")))
        .collect()
}

#[test]
fn whether_a_route_no_section_reaches_is_sent_again_is_the_same_however_many_advisories() {
    let config = configured(true, r#"["advisories"]"#, LOBBY_ROUTE);
    let room = [(CLIENT, "invite")];
    let measured = held(&room, (&listed(0), BTreeMap::new()), &["$a", "$b"]);
    round(&config, &healthy(), &measured, NOON);
    let most = measured.made().load(Ordering::Relaxed);
    let answers = &["$a", "$b", "$c", "$d", "$e", "$f", "$g", "$h"];
    let mut sent = Vec::new();
    for count in [0, 400] {
        let store = healthy();
        let first = held(&room, (&listed(count), BTreeMap::new()), answers);
        let said = format!("{:?}", bounded(&config, &store, &first, (NOON, Some(most))));
        assert_eq!(
            said.contains("Sealed { stopped: true }"),
            count > 0,
            "{said}"
        );
        let second = held(&room, (&listed(count), renamed()), answers);
        let later = NOON + SignedDuration::from_mins(10);
        bounded(&config, &store, &second, (later, None));
        sent.push((lobby(&first), lobby(&second)));
    }
    assert_eq!(sent[0].0.len(), 1, "{sent:?}");
    assert_eq!(sent[0], sent[1]);
}

#[test]
fn a_recorded_post_in_a_place_now_public_loses_its_head_however_many_plain_posts_wait() {
    let boards: String = (1..=4)
        .map(|board| LOBBY.replace("lobby", &format!("lobby{board}")))
        .collect();
    let config = configured(true, r#"["advisories"]"#, &boards);
    let store = healthy();
    let first = held(
        &[(CLIENT, "invite")],
        (&alerts(true), BTreeMap::new()),
        &["$p1", "$p2", "$p3", "$p4", "$one"],
    );
    round(&config, &store, &first, NOON);
    let opened = held(
        &[(CLIENT, "public")],
        (&alerts(true), renamed()),
        &["$x1", "$x2", "$x3", "$x4", "$x5", "$x6"],
    );
    let later = NOON + SignedDuration::from_hours(1);
    bounded(&config, &store, &opened, (later, Some(6)));
    let seen = opened.seen();
    assert!(
        seen.iter()
            .any(|line| line.starts_with(&format!("PUT {CLIENT}/redact/%24one/"))),
        "{seen:?}"
    );
    assert!(lobby(&opened).len() < 4, "{seen:?}");
}
