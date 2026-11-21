//! A report's round at its edges: routes shown alike sent alike, the
//! posts still waiting for the section kept their requests, a route still
//! pending sent alone, and a round that cannot read its cursor or render
//! its report stopping (ADR-0067).

use std::collections::BTreeMap;

use herald_flows::{Stop, Sweep, Swept};

use crate::advisories::{
    ALERT_PAGE, CHANNEL_READ, CODE, DEPENDABOT, GRAPHQL, GUILD_READ, ONBOARDING_READ, SECRETS,
    WITHHELD, alerts, answers, config, page, posted, reported, round, routed,
};
use crate::harness::{Fault, NOON, PINNED, POSTED, Script, WHO, healthy};
use crate::replaced::answer;

fn stopped(swept: &[Swept]) -> Vec<&Stop> {
    swept
        .iter()
        .filter_map(|swept| match &swept.outcome {
            Sweep::Stopped(stop) => Some(stop),
            _ => None,
        })
        .collect()
}

#[test]
fn routes_shown_alike_are_sent_alike_and_none_is_read_an_advisory() {
    let config = config(&format!(
        "{}{}{}",
        reported("daily"),
        routed("daily", "lobby", false),
        routed("daily", "hall", false)
    ));
    let page = page();
    let script = Script::answering(&[(GRAPHQL, 200, &page), POSTED, POSTED]);
    round(&config, (&healthy(), &script), (NOON, None));
    let bodies = posted(&script);
    assert_eq!(bodies.len(), 2, "{:?}", script.seen());
    assert!(
        bodies.iter().all(|body| body.contains(WITHHELD)),
        "{bodies:?}"
    );
    assert!(
        script.seen().iter().all(|line| !line.contains("/alerts?")),
        "{:?}",
        script.seen()
    );
}

#[test]
fn the_requests_every_step_of_a_waiting_post_takes_are_kept_from_the_advisories_read() {
    let pinned = "[[routes]]\nid = \"team\"\nsources = [\"daily\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"herald:queue\"\nprivate = true\nliving = true\npin = true\n";
    let config = config(&format!(
        "{}{pinned}{}",
        reported("daily"),
        routed("daily", "lobby", false)
    ));
    let page = page();
    let script = Script::answering(&[
        (GRAPHQL, 200, &page),
        CHANNEL_READ,
        GUILD_READ,
        ONBOARDING_READ,
        POSTED,
        (CODE, 200, "[]"),
        POSTED,
        PINNED,
    ]);
    round(&config, (&healthy(), &script), (NOON, Some(9)));
    let seen = script.seen();
    assert!(
        seen.iter().all(|line| !line.contains("dependabot")),
        "{seen:?}"
    );
    let bodies = posted(&script);
    assert_eq!(bodies.len(), 2, "{seen:?}");
    assert!(bodies[0].contains(WITHHELD), "{}", bodies[0]);
    assert!(
        bodies[1].contains("budget ended before its advisories were read"),
        "{}",
        bodies[1]
    );
}

/// The round's every answer by its URL, whatever the order: the view's
/// page, the channel found private, its listings of alerts, Dependabot's
/// holding one, the channel's messages read back holding no earlier post,
/// and each post.
fn keyed(page: &str) -> Script {
    let discord = "https://discord.com/api/v10";
    let github = "https://api.github.com";
    let open = alerts(true);
    let mut keyed = BTreeMap::new();
    for (url, body) in [
        (GRAPHQL.to_owned(), page),
        (format!("{discord}{}", CHANNEL_READ.0), CHANNEL_READ.2),
        (format!("{discord}{}", GUILD_READ.0), GUILD_READ.2),
        (format!("{discord}{}", ONBOARDING_READ.0), ONBOARDING_READ.2),
        (format!("{github}{CODE}"), "[]"),
        (format!("{github}{DEPENDABOT}"), open.as_str()),
        (format!("{github}{SECRETS}"), "[]"),
        (format!("{discord}{}", WHO.0), WHO.2),
        (format!("{discord}{}?limit=100", POSTED.0), "[]"),
        (format!("{discord}{}", POSTED.0), POSTED.2),
    ] {
        keyed.insert(url, answer(body));
    }
    Script::keyed(keyed)
}

#[test]
fn a_route_whose_section_stopped_is_sent_alone_at_the_next_round() {
    let config = config(&format!(
        "{}{}{}",
        reported("daily"),
        routed("daily", "team", true),
        routed("daily", "lobby", false)
    ));
    let store = healthy();
    let (page, open) = (page(), alerts(true));
    let refused = (
        POSTED.0,
        403,
        r#"{"code":50013,"message":"Missing Permissions"}"#,
    );
    let mut first = answers((&page, &open), &[refused]);
    first.insert(4, POSTED);
    let first = Script::answering(&first);
    round(&config, (&store, &first), (NOON, None));
    assert_eq!(posted(&first).len(), 2, "{:?}", first.seen());
    let again = keyed(&page);
    round(&config, (&store, &again), (NOON, None));
    let bodies = posted(&again);
    assert_eq!(bodies.len(), 1, "{:?}", again.seen());
    assert!(bodies[0].contains(ALERT_PAGE), "{}", bodies[0]);
}

#[test]
fn a_cursor_that_cannot_be_read_again_stops_the_round_once_the_parts_no_section_reaches_went_out() {
    let config = config(&format!(
        "{}{}",
        reported("daily"),
        routed("daily", "lobby", false)
    ));
    let mut store = healthy();
    store.fault = Fault::ReadsCursorOnce;
    let page = page();
    let script = Script::answering(&[(GRAPHQL, 200, &page), POSTED]);
    let swept = round(&config, (&store, &script), (NOON, None));
    assert!(matches!(stopped(&swept)[..], [Stop::Store(_)]), "{swept:?}");
    let bodies = posted(&script);
    assert_eq!(bodies.len(), 1, "{:?}", script.seen());
    assert!(bodies[0].contains(WITHHELD), "{}", bodies[0]);
}

#[test]
fn a_report_whose_name_renders_no_title_stops_before_any_post() {
    let blank = |private: bool| {
        config(&format!(
            "[reports.\" \"]\nview = \"security\"\nevery = \"1h\"\nat = \"00:00\"\n[[routes]]\nid = \"posted\"\nsources = [\" \"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"herald:queue\"\nprivate = {private}\nliving = {private}\npin = false\n"
        ))
    };
    let (page, open) = (page(), alerts(true));
    let plain = Script::answering(&[(GRAPHQL, 200, &page)]);
    let swept = round(&blank(false), (&healthy(), &plain), (NOON, None));
    assert!(
        matches!(stopped(&swept)[..], [Stop::Source(_)]),
        "{swept:?}"
    );
    let shown = Script::answering(&answers((&page, &open), &[]));
    let swept = round(&blank(true), (&healthy(), &shown), (NOON, None));
    assert!(
        matches!(stopped(&swept)[..], [Stop::Source(_)]),
        "{swept:?}"
    );
    assert_eq!(posted(&shown), Vec::<String>::new());
}

#[test]
fn a_cursor_written_first_by_another_run_withholds_the_section() {
    let store = crate::harness::Failing {
        fault: Fault::CursorConflicts,
        ..healthy()
    };
    let (page, open) = (page(), alerts(true));
    let script = Script::answering(&answers((&page, &open), &[POSTED]));
    round(
        &config(&crate::advisories::board(true)),
        (&store, &script),
        (NOON, None),
    );
    let body = posted(&script).pop().unwrap();
    assert!(
        body.contains(WITHHELD) && !body.contains(ALERT_PAGE),
        "{body}"
    );
}

#[test]
fn a_living_post_already_recorded_is_shown_the_section_with_no_name_written_again() {
    let mut store = healthy();
    crate::advised::boarded(&store);
    store.fault = Fault::RecallsConflict;
    let page = page();
    let later = keyed(&page);
    round(
        &config(&crate::advisories::board(true)),
        (&store, &later),
        (NOON + jiff::SignedDuration::from_hours(1), None),
    );
    let bodies = posted(&later);
    assert!(
        bodies.iter().all(|body| !body.contains(WITHHELD)),
        "{bodies:?}"
    );
}
