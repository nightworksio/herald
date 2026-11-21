//! A view's advisories overview: read once a round through the run's
//! budget, shown to routes that say `private = true` alone, and to every
//! other route, living or not, replaced by one line that is the same
//! whatever the section held (ADR-0067).

use herald_config::{Config, Uses, load};
use herald_flows::{Budget, Connected, Ports, Requests, Swept, prepare, queue, report};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use crate::harness::{Failing, NOON, POSTED, Script, healthy};

pub(crate) const GRAPHQL: &str = "https://api.github.com/graphql";
pub(crate) const CODE: &str = "/repos/o/open/code-scanning/alerts?state=open&per_page=100";
pub(crate) const DEPENDABOT: &str = "/repos/o/open/dependabot/alerts?state=open&per_page=100";
pub(crate) const SECRETS: &str =
    "/repos/o/open/secret-scanning/alerts?state=open&per_page=100&hide_secret=true";
pub(crate) const ALERT_PAGE: &str = "https://github.com/o/open/security/dependabot/3";
pub(crate) const WITHHELD: &str = "withheld from this route because it is not private";
pub(crate) const EDITED: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages/1",
    200,
    r#"{"id":"1"}"#,
);

pub(crate) fn config(routes: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.open]
kind = "github"
repository = "o/open"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[views.security]
sources = ["open"]
overviews = ["advisories"]
[run]
routes = 1
{routes}
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

/// A route of the report `report` to the channel, private and keeping a
/// living post where `private` says, as only a living post carries the
/// section.
pub(crate) fn routed(report: &str, id: &str, private: bool) -> String {
    format!(
        "[[routes]]\nid = \"{id}\"\nsources = [\"{report}\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"herald:queue\"\nprivate = {private}\nliving = {private}\npin = false\n"
    )
}

pub(crate) fn reported(name: &str) -> String {
    format!("[reports.{name}]\nview = \"security\"\nevery = \"1h\"\nat = \"00:00\"\n")
}

pub(crate) fn board(private: bool) -> String {
    format!(
        "[boards.live]\nview = \"security\"\nevery = \"1h\"\nat = \"00:00\"\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"herald:queue\"\npin = false\nprivate = {private}\n"
    )
}

pub(crate) fn page() -> String {
    json!({ "data": { "repository": { "visibility": "PUBLIC", "pullRequests": { "nodes": [ {
        "number": 1, "title": "Change", "url": "https://github.com/o/open/pull/1",
        "isDraft": false, "createdAt": "2026-10-20T10:00:00Z", "updatedAt": "2026-10-30T11:00:00Z",
        "headRefOid": "h1",
        "author": { "__typename": "User", "login": "ana" },
        "reviewDecision": "REVIEW_REQUIRED",
        "reviewRequests": { "totalCount": 0, "nodes": [] }
    } ] } } } })
    .to_string()
}

pub(crate) fn alerts(open: bool) -> String {
    let alerts = if open {
        json!([{
            "number": 3, "state": "open", "html_url": ALERT_PAGE,
            "created_at": "2026-10-01T10:00:00Z",
            "security_advisory": { "summary": "Prototype pollution", "severity": "critical" },
        }])
    } else {
        json!([])
    };
    alerts.to_string()
}

/// Discord's answer for the routes' channel, which the server's
/// `@everyone` may not view.
pub(crate) const CHANNEL_READ: (&str, u16, &str) = (
    "/channels/1566200000000000100",
    200,
    r#"{"id":"1566200000000000100","type":0,"guild_id":"1566200000000000900","permission_overwrites":[{"id":"1566200000000000900","type":0,"allow":"0","deny":"1024"}]}"#,
);
/// Discord's answer for the routes' server, whose `@everyone` may view
/// its channels unless one says otherwise.
pub(crate) const GUILD_READ: (&str, u16, &str) = (
    "/guilds/1566200000000000900",
    200,
    r#"{"id":"1566200000000000900","roles":[{"id":"1566200000000000900","name":"@everyone","permissions":"1024"}]}"#,
);
/// Discord's answer for the routes' server's onboarding, which hands out
/// neither the channel nor a role.
pub(crate) const ONBOARDING_READ: (&str, u16, &str) = (
    "/guilds/1566200000000000900/onboarding",
    200,
    r#"{"guild_id":"1566200000000000900","enabled":false,"default_channel_ids":[],"prompts":[]}"#,
);

/// The view's page and its three listings of alerts, Dependabot's holding
/// one where `open` says, then `then`: what `herald queue` reads.
pub(crate) fn listed<'a>(
    (page, open): (&'a str, &'a str),
    then: &[(&'a str, u16, &'a str)],
) -> Vec<(&'a str, u16, &'a str)> {
    let mut answers = vec![
        (GRAPHQL, 200, page),
        (CODE, 200, "[]"),
        (DEPENDABOT, 200, open),
        (SECRETS, 200, "[]"),
    ];
    answers.extend_from_slice(then);
    answers
}

/// A round's answers: the view's page, the private routes' channel found
/// private, its three listings of alerts, Dependabot's holding one where
/// `open` says, then `then`.
pub(crate) fn answers<'a>(
    read: (&'a str, &'a str),
    then: &[(&'a str, u16, &'a str)],
) -> Vec<(&'a str, u16, &'a str)> {
    let mut answers = listed(read, then);
    answers.splice(1..1, [CHANNEL_READ, GUILD_READ, ONBOARDING_READ]);
    answers
}

/// A round's answers where one route is shown the section and one is not:
/// the view's page, the private route's channel checked, the post to the
/// route that is not, then the three listings, Dependabot's holding one where `open` says, then the post to
/// the private route.
fn mixed<'a>((page, open): (&'a str, &'a str)) -> Vec<(&'a str, u16, &'a str)> {
    let mut answers = answers((page, open), &[POSTED]);
    answers.insert(4, POSTED);
    answers
}

/// The board `herald queue` reads of the view `security`, its output
/// private where `private` says.
fn queued(config: &Config, script: &Script, private: bool) -> herald_core::Board {
    let secrets = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "hunter2")
        .with("HERALD_GITHUB_TOKEN", "ghp_x");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let store = healthy();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    pollster::block_on(queue(
        config,
        &connected,
        &ports,
        ("security", herald_core::Printed::of(private, true)),
    ))
    .unwrap()
}

pub(crate) fn round(
    config: &Config,
    (store, script): (&Failing, &Script),
    (at, most): (Timestamp, Option<usize>),
) -> Vec<Swept> {
    let secrets = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "hunter2")
        .with(
            "HERALD_DISCORD_WEBHOOK",
            "https://discord.com/api/webhooks/1/hook",
        )
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
    pollster::block_on(report(config, &connected, &ports, "r1"))
}

/// `body` as its destination was sent it, but for the nonce each post
/// draws anew.
fn unnonced(body: &str) -> String {
    let mut sent: serde_json::Value = serde_json::from_str(body).unwrap();
    sent.as_object_mut().unwrap().remove("nonce");
    sent.to_string()
}

pub(crate) fn posted(script: &Script) -> Vec<String> {
    script
        .bodies()
        .into_iter()
        .filter(|body| !body.is_empty())
        .map(|body| String::from_utf8(body).unwrap())
        .filter(|body| body.contains("content") || body.contains("embeds"))
        .collect()
}

#[test]
fn a_private_route_is_shown_the_section_and_any_other_the_line_byte_for_byte_as_with_none() {
    let config = config(&format!(
        "{}{}{}",
        reported("daily"),
        routed("daily", "team", true),
        routed("daily", "lobby", false)
    ));
    let page = page();
    let (open, none) = (alerts(true), alerts(false));
    let script = Script::answering(&mixed((&page, &open)));
    round(&config, (&healthy(), &script), (NOON, None));
    let bodies = posted(&script);
    assert_eq!(bodies.len(), 2, "{:?}", script.seen());
    let (public, private) = (&bodies[0], &bodies[1]);
    assert!(private.contains(ALERT_PAGE), "{private}");
    assert!(private.contains("Prototype pollution"), "{private}");
    assert!(!private.contains(WITHHELD), "{private}");
    assert!(public.contains(WITHHELD), "{public}");
    for leaked in [ALERT_PAGE, "Prototype pollution", "advisories", "critical"] {
        assert!(!public.contains(leaked), "{leaked} in {public}");
    }
    let clean = Script::answering(&mixed((&page, &none)));
    round(&config, (&healthy(), &clean), (NOON, None));
    let clean = posted(&clean);
    assert_eq!(unnonced(&clean[0]).as_bytes(), unnonced(public).as_bytes());
    assert_ne!(unnonced(&clean[1]), unnonced(private));
    assert!(private.contains("30d old"), "{private}");
}

#[test]
fn two_reports_of_one_view_read_it_once_and_each_route_is_shown_as_it_says() {
    let config = config(&format!(
        "{}{}{}{}",
        reported("a"),
        reported("b"),
        routed("a", "team", true),
        routed("b", "lobby", false)
    ));
    let (page, open) = (page(), alerts(true));
    let script = Script::answering(&mixed((&page, &open)));
    round(&config, (&healthy(), &script), (NOON, None));
    let listed = script
        .seen()
        .iter()
        .filter(|line| line.contains("/alerts?"))
        .count();
    assert_eq!(listed, 3);
    let bodies = posted(&script);
    assert!(bodies[1].contains(ALERT_PAGE), "{bodies:?}");
    assert!(bodies[0].contains(WITHHELD), "{bodies:?}");
    assert!(!bodies[0].contains(ALERT_PAGE), "{bodies:?}");
}

#[test]
fn a_living_post_whose_route_stops_being_private_loses_the_section_at_its_next_edit() {
    let store = healthy();
    let (page, open) = (page(), alerts(true));
    let first = Script::answering(&answers((&page, &open), &[POSTED]));
    round(&config(&board(true)), (&store, &first), (NOON, None));
    assert!(posted(&first)[0].contains(ALERT_PAGE));
    let later = NOON.checked_add(SignedDuration::from_hours(1)).unwrap();
    let edited = Script::answering(&[(GRAPHQL, 200, &page), EDITED, EDITED]);
    round(&config(&board(false)), (&store, &edited), (later, None));
    let lines = edited.seen();
    assert!(
        lines[1].starts_with("PATCH ") && lines[2].starts_with("PATCH "),
        "{lines:?}"
    );
    assert!(
        lines.iter().all(|line| !line.contains("/alerts?")),
        "{lines:?}"
    );
    let edits = posted(&edited);
    assert!(
        edits[0].contains("removed because this route no longer receives it"),
        "{}",
        edits[0]
    );
    assert!(edits[1].contains(WITHHELD), "{}", edits[1]);
    for edit in edits {
        assert!(!edit.contains(ALERT_PAGE), "{edit}");
    }
}

#[test]
fn a_budget_ending_in_the_advisories_read_names_them_unread_and_spares_every_post() {
    let config = config(&format!(
        "{}{}{}",
        reported("daily"),
        routed("daily", "team", true),
        routed("daily", "lobby", false)
    ));
    let page = page();
    let bounded = Script::answering(&[
        (GRAPHQL, 200, &page),
        CHANNEL_READ,
        GUILD_READ,
        ONBOARDING_READ,
        POSTED,
        (CODE, 200, "[]"),
        POSTED,
    ]);
    let swept = round(&config, (&healthy(), &bounded), (NOON, Some(7)));
    assert!(
        matches!(&swept[0].outcome, herald_flows::Sweep::Reported { .. }),
        "{swept:?}"
    );
    let bodies = posted(&bounded);
    assert_eq!(bodies.len(), 2, "{:?}", bounded.seen());
    assert!(bodies[0].contains(WITHHELD), "{}", bodies[0]);
    assert!(
        bodies[1].contains("budget ended before its advisories were read"),
        "{}",
        bodies[1]
    );
    for (failing, why) in [
        (
            Script::answering(&[
                (GRAPHQL, 200, &page),
                CHANNEL_READ,
                GUILD_READ,
                ONBOARDING_READ,
                POSTED,
                (CODE, 500, "{}"),
                POSTED,
            ]),
            "its advisories could not be read",
        ),
        (
            Script::answering(&[
                (GRAPHQL, 200, &page),
                CHANNEL_READ,
                GUILD_READ,
                ONBOARDING_READ,
                POSTED,
            ]),
            "its forge could not be reached",
        ),
    ] {
        round(&config, (&healthy(), &failing), (NOON, None));
        let body = posted(&failing).pop().unwrap_or_default();
        assert!(body.contains(why), "{why}\n{body}\n{:?}", failing.seen());
    }
}

#[test]
fn every_report_s_routes_no_section_reaches_are_posted_before_any_advisory_is_read() {
    let config = config(&format!(
        "{}{}{}{}{}{}",
        reported("daily"),
        reported("weekly"),
        routed("daily", "team", true),
        routed("daily", "lobby", false),
        routed("weekly", "crew", true),
        routed("weekly", "hall", false)
    ));
    let page = page();
    let open = alerts(true);
    let script = Script::answered(
        [
            (GRAPHQL, 200, page.clone(), Vec::new()),
            (CHANNEL_READ.0, 200, CHANNEL_READ.2.to_owned(), Vec::new()),
            (GUILD_READ.0, 200, GUILD_READ.2.to_owned(), Vec::new()),
            (ONBOARDING_READ.0, 200, ONBOARDING_READ.2.to_owned(), Vec::new()),
            ("/channels/1566200000000000100/messages", 200, POSTED.2.to_owned(), Vec::new()),
            ("/channels/1566200000000000100/messages", 200, POSTED.2.to_owned(), Vec::new()),
            (
                CODE,
                200,
                open,
                vec![(
                    "Link".to_owned(),
                    "<https://api.github.com/repos/o/open/code-scanning/alerts?page=2>; rel=\"next\"".to_owned(),
                )],
            ),
            ("/channels/1566200000000000100/messages", 200, POSTED.2.to_owned(), Vec::new()),
            ("/channels/1566200000000000100/messages", 200, POSTED.2.to_owned(), Vec::new()),
        ]
        .into_iter()
        .map(|(line, status, body, headers)| {
            (
                line.to_owned(),
                herald_ports::Response {
                    status,
                    headers,
                    body: body.into_bytes(),
                },
            )
        })
        .collect(),
    );
    round(&config, (&healthy(), &script), (NOON, Some(9)));
    let seen = script.seen();
    assert_eq!(seen.len(), 9, "{seen:?}");
    let bodies = posted(&script);
    assert_eq!(bodies.len(), 4, "{seen:?}");
    assert!(
        bodies[0].contains(WITHHELD) && bodies[1].contains(WITHHELD),
        "{bodies:?}"
    );
    for private in &bodies[2..] {
        assert!(!private.contains(WITHHELD), "{private}");
        assert!(private.contains("budget ended"), "{private}");
    }
}

#[test]
fn a_report_none_of_whose_routes_is_private_reads_no_advisory() {
    let config = config(&format!(
        "{}{}",
        reported("daily"),
        routed("daily", "lobby", false)
    ));
    let page = page();
    let script = Script::answering(&[(GRAPHQL, 200, &page), POSTED]);
    round(&config, (&healthy(), &script), (NOON, None));
    assert!(
        script.seen().iter().all(|line| !line.contains("/alerts?")),
        "{:?}",
        script.seen()
    );
    assert!(posted(&script)[0].contains(WITHHELD));
}

#[test]
fn herald_queue_shows_the_section_only_where_the_operator_says_its_output_is_private() {
    let config = config("");
    let (page, open) = (page(), alerts(true));
    for private in [true, false] {
        let script = Script::answering(&listed((&page, &open), &[]));
        let board = queued(&config, &script, private);
        let shown = board.overviews.advisories.unwrap();
        assert_eq!(
            shown.shown().map(|found| found.open.len()),
            private.then_some(1),
            "{private}"
        );
        assert_eq!(
            script.seen().len(),
            if private { 4 } else { 1 },
            "{:?}",
            script.seen()
        );
    }
}
