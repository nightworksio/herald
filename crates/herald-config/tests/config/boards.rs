//! `[rules]`, `[boards]` and a route's `living` (ADR-0056): rules and
//! boards read as reports are, a board's living route read as any route,
//! and every mistake named by its key.

use herald_config::{Config, NUDGE_TEMPLATE, QUEUE_TEMPLATE, load};
use herald_core::{
    Dequeued, Kind, MOST_NUDGES, MOST_RUNS, MOST_RUNS_ALLOWED, Pinning, SourceName, Stages,
    TemplateName,
};
use jiff::SignedDuration;
use jiff::civil::Time;

const EXAMPLE: &str = r#"version = 1

[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[destinations.slack]
kind = "slack"
webhook = { secret = "HERALD_SLACK_WEBHOOK" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
"#;

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn with(tables: &str) -> String {
    format!("{EXAMPLE}\n{tables}")
}

fn loaded(text: &str) -> Config {
    load("herald.toml", text, &nothing, &crate::common::kinds()).unwrap()
}

fn errors(text: &str) -> String {
    load("herald.toml", text, &nothing, &crate::common::kinds())
        .unwrap_err()
        .to_string()
}

/// A route's id, template, whether it lives, how it pins, and whether
/// its place is private.
type Delivered<'a> = (&'a str, Option<&'a str>, bool, Option<Pinning>, bool);

fn days(count: i64) -> SignedDuration {
    SignedDuration::from_hours(count * 24)
}

#[test]
fn a_rule_watches_a_view_when_a_report_would_and_nudges_past_its_stages() {
    let config = loaded(&with(
        r#"[rules.review]
stages = ["7d", "2d"]
at = "08:30"
most = 5

[rules.plain]
stages = ["1d"]

[[routes]]
id = "nudges"
sources = ["review"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:nudge"
"#,
    ));
    let review = &config.rules["review"];
    assert_eq!(review.watch.view, "queue");
    assert_eq!(review.watch.schedule.at, Time::constant(8, 30, 0, 0));
    assert_eq!(
        review.stages.as_ref().map(Stages::as_slice),
        Some([days(2), days(7)].as_slice())
    );
    assert!(!review.rerun);
    assert_eq!(review.most, 5);
    assert_eq!(config.rules["plain"].most, MOST_NUDGES);
    assert_eq!(
        config.routes[0].route.template,
        Some(TemplateName::new(NUDGE_TEMPLATE))
    );
    assert!(config.reports.is_empty());
}

#[test]
fn a_route_naming_a_rule_takes_its_nudges_alone() {
    let error = errors(&with(
        r#"[rules.review]
stages = ["2d"]

[[routes]]
id = "nudges"
sources = ["review"]
kinds = ["report"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:nudge"
"#,
    ));
    assert!(error.contains("routes[0].kinds[0]"), "{error}");
    let config = loaded(&with(
        r#"[rules.review]
stages = ["2d"]

[[routes]]
id = "nudges"
sources = ["review"]
kinds = ["nudge"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:nudge"
"#,
    ));
    assert_eq!(config.routes[0].route.filter.kinds, [Kind::NUDGE]);
}

#[test]
fn every_mistake_in_a_rule_is_named_by_its_key() {
    let error = errors(&with(
        r#"[reports.daily]

[rules.none]
stages = []

[rules.twice]
stages = ["2d", "48h"]

[rules.fine]
stages = ["500ms"]

[rules.daily]
stages = ["2d"]

[rules.gate]
stages = ["2d"]

[rules.lost]
stages = ["2d"]
view = "missing"
every = "2d"

[rules.odd]
stages = ["2d"]
unknown = true

[rules.bare]
most = 3

[boards]
flat = 3
"#,
    ));
    for wanted in [
        "rules.none.stages: a rule names from 1 to 16 stages, each a whole second or longer and none twice, such as stages = [\"2d\", \"7d\"]",
        "rules.twice.stages: a rule names from 1 to 16 stages",
        "rules.fine.stages: a rule names from 1 to 16 stages",
        "rules.daily: a source, app, report or rule is named \"daily\" too",
        "rules.gate: a source, app, report or rule is named \"gate\" too",
        "rules.lost.view: no view is named \"missing\"; the views are \"queue\"",
        "rules.lost.every: a report's window lasts from \"1m\" to \"1d\"",
        "rules.odd",
        "rules.bare",
        "boards.flat: boards.flat is a table; write [boards.flat]",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
}

#[test]
fn a_rule_written_as_no_table_is_named() {
    let error = errors(&with("[rules]\nflat = 3\n"));
    assert!(
        error.contains("rules.flat: rules.flat is a table; write [rules.flat]"),
        "{error}"
    );
}

#[test]
fn a_board_is_a_report_and_a_living_route_pinned_where_its_destination_pins() {
    let config = loaded(&with(
        r#"[boards.queue]
every = "1h"
destination = "discord"
channel = "1566200000000000100"

[boards.plain]
destination = "slack"
template = "plain"
private = true

[templates.plain]
title = "{{ view.name }}"
body = "{{ view.total }}"
"#,
    ));
    let report = &config.reports["queue"];
    assert_eq!(report.view, "queue");
    assert_eq!(report.schedule.every, SignedDuration::from_hours(1));
    let routes: Vec<Delivered<'_>> = config
        .routes
        .iter()
        .map(|route| {
            (
                route.route.id.as_str(),
                route.route.template.as_ref().map(TemplateName::as_str),
                route.route.delivery.living,
                route.route.delivery.pin,
                route.private,
            )
        })
        .collect();
    assert_eq!(
        routes,
        [
            ("plain", Some("plain"), true, None, true),
            (
                "queue",
                Some(QUEUE_TEMPLATE),
                true,
                Some(Pinning::Markers),
                false
            ),
        ]
    );
    assert_eq!(
        config.routes[0].route.filter.sources,
        [SourceName::new("plain")]
    );
}

#[test]
fn a_route_pins_by_true_false_or_how_it_finds_its_earlier_pin() {
    let route = |pin: &str| {
        with(&format!(
            r#"[reports.daily]

[[routes]]
id = "board"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:queue"
living = true
{pin}
"#
        ))
    };
    let pins: Vec<Option<Pinning>> = ["", "pin = true", "pin = false", "pin = \"store\""]
        .into_iter()
        .map(|pin| {
            let config = loaded(
                &route(pin)
                    .replace("kind = \"git-branch\"", "kind = \"sqlite\"")
                    .replace("key = { secret = \"HERALD_STORE_KEY\" }\n", ""),
            );
            config.routes[0].route.delivery.pin
        })
        .collect();
    assert_eq!(
        pins,
        [
            Some(Pinning::Markers),
            Some(Pinning::Markers),
            None,
            Some(Pinning::Store)
        ]
    );
    let unliving = loaded(&route("").replace("living = true\n", ""));
    assert_eq!(unliving.routes[0].route.delivery.pin, None);
    assert!(!unliving.routes[0].route.delivery.living);
}

#[test]
fn a_living_route_names_reports_alone_and_a_board_names_no_key_it_gives_itself() {
    let error = errors(&with(
        r#"[reports.daily]

[reports.board]

[[routes]]
id = "living"
sources = ["daily", "gate"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:queue"
living = true

[[routes]]
id = "taken"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:queue"

[boards.named]
id = "other"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"

[boards.board]
destination = "discord"
channel = "1566200000000000100"

[boards.taken]
destination = "discord"
channel = "1566200000000000100"

[boards.nowhere]
destination = "missing"
zone = "Mars/Olympus"
"#,
    ));
    for wanted in [
        "routes[0].living: a living route keeps one post of each report it takes, so it names only reports among its sources; \"gate\" is not one",
        "boards.named: a board is its own living route to its own report, so it names no id, sources",
        "boards.board: a source, app, report or rule is named \"board\" too",
        "boards.taken.id: another route has the id \"taken\"; give each route its own",
        "boards.nowhere.destination: no destination is named \"missing\"",
        "boards.nowhere.zone: there is no time zone \"Mars/Olympus\"",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
}

#[test]
fn a_rule_plans_re_runs_of_flaky_checks_and_never_applies_them_itself() {
    let config = loaded(&with(
        r#"[rules.flaky]
flaky = { rerun = true, mode = "plan" }

[rules.both]
stages = ["2d"]
flaky = { rerun = true, runs = 2 }

[rules.most]
flaky = { rerun = true, runs = 100 }

[[routes]]
id = "plans"
sources = ["flaky"]
kinds = ["rerun"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:rerun"
private = true
"#,
    ));
    let flaky = &config.rules["flaky"];
    assert_eq!(
        (flaky.stages.as_ref(), flaky.rerun, flaky.runs),
        (None, true, MOST_RUNS)
    );
    assert_eq!(config.rules["both"].runs, 2);
    assert_eq!(config.rules["most"].runs, MOST_RUNS_ALLOWED);
    assert!(config.rules["both"].stages.is_some() && config.rules["both"].rerun);
    assert_eq!(config.routes[0].route.filter.kinds, [Kind::RERUN]);
    let error = errors(&with(
        r#"[rules.apply]
flaky = { rerun = true, mode = "apply" }

[rules.idle]
flaky = { rerun = false }

[rules.wide]
flaky = { rerun = true, runs = 101 }
"#,
    ));
    for wanted in [
        "rules.apply.flaky.mode: a rule only plans a re-run, so mode is \"plan\"; a re-run is made by an operator signed in through the forge, never by a rule on its own",
        "rules.wide.flaky.runs: a plan reads from 1 to 100 failed runs on a head, such as runs = 5",
        "rules.idle: a rule nudges at its stages, plans re-runs of flaky checks with flaky = { rerun = true }, alerts on pull requests leaving the merge queue with dequeued = true, alerts on its view's release with release = { ready = true, late = 7 }, or any of them; this one does none",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
}

#[test]
fn a_rule_alerts_on_pull_requests_leaving_the_merge_queue_for_the_reasons_it_names() {
    let config = loaded(&with(
        r#"[rules.failed]
dequeued = true

[rules.named]
stages = ["2d"]
dequeued = ["removed", "checks-failed", "removed"]

[[routes]]
id = "alerts"
sources = ["failed"]
kinds = ["dequeued"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:dequeued"
private = true
"#,
    ));
    assert_eq!(config.rules["failed"].dequeued, Dequeued::FAILURES);
    assert_eq!(
        config.rules["named"].dequeued,
        [Dequeued::ChecksFailed, Dequeued::Removed]
    );
    assert_eq!(config.routes[0].route.filter.kinds, [Kind::DEQUEUED]);
    let error = errors(&with(
        "[rules.off]\ndequeued = false\n\n[rules.none]\ndequeued = []\n\n[rules.odd]\ndequeued = [\"flaked\"]\n",
    ));
    for wanted in [
        "rules.off: a rule nudges at its stages",
        "rules.none: a rule nudges at its stages",
        "rules.odd",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
}

#[test]
fn a_rule_alerts_on_its_view_s_release_turning_ready_or_late() {
    let views = r#"[views.ship]
sources = ["gate"]
release = { milestone = "v2.0", header = true }

[views.plain]
sources = ["gate"]
release = { label = "ship" }
"#;
    let config = loaded(&with(&format!(
        r#"{views}
[rules.ready]
view = "ship"
release = {{ ready = true, late = 7 }}

[rules.edge]
view = "ship"
release = {{ late = 365 }}

[rules.soon]
view = "ship"
release = {{ late = 0 }}

[[routes]]
id = "alerts"
sources = ["ready"]
kinds = ["readiness"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:readiness"
"#
    )));
    let alerting = |ready, late| herald_core::Alerting { ready, late };
    assert_eq!(config.rules["ready"].release, alerting(true, Some(7)));
    assert_eq!(config.rules["edge"].release, alerting(false, Some(365)));
    assert_eq!(config.rules["soon"].release, alerting(false, Some(0)));
    assert_eq!(config.routes[0].route.filter.kinds, [Kind::READINESS]);
    let error = errors(&with(&format!(
        r#"{views}
[rules.unreleased]
release = {{ ready = true }}

[rules.undated]
view = "plain"
release = {{ late = 3 }}

[rules.far]
view = "ship"
release = {{ late = 366 }}

[rules.off]
view = "ship"
release = {{ ready = false }}
"#
    )));
    for wanted in [
        "rules.unreleased.release: a rule's release alerts judge the release of the view it watches, so that view needs one",
        "rules.undated.release.late: release.late reads the due days of the milestones the release's selector matches, so the view's release needs header = true or rows = \"issues\"",
        "rules.far.release.late: release.late is from 0 to 365 days, such as late = 7",
        "rules.off: a rule nudges at its stages, plans re-runs of flaky checks with flaky = { rerun = true }, alerts on pull requests leaving the merge queue with dequeued = true, alerts on its view's release with release = { ready = true, late = 7 }, or any of them; this one does none",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
}
