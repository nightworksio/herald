//! A living board keeps one post, edited each window where its destination
//! edits and posted anew where it cannot; a rule nudges each pull request
//! that waited past a stage once a stage and spell, reading each view once
//! a round with the reports (ADR-0056).

use herald_config::{Config, Uses, load};
use herald_core::SourceName;
use herald_flows::{
    Budget, Connected, Outcome, Ports, Remembered, Requests, Stop, Sweep, Swept, prepare, report,
};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use crate::harness::{Failing, NO_PINS, NOON, PINNED, POSTED, Script, WHO, healthy};

const GRAPHQL: &str = "https://api.github.com/graphql";
const EDITED: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages/1",
    200,
    r#"{"id":"1"}"#,
);
const HOOK: &str = "https://hooks.slack.com/services/T0/B0/x";

fn config(extra: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[destinations.slack]
kind = "slack"
webhook = {{ secret = "HERALD_SLACK_WEBHOOK" }}
[sources.open]
kind = "github"
repository = "o/open"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[views.open]
sources = ["open"]
[run]
routes = 1
[templates.t]
title = "{{{{ view.name }}}}"
body = "rows:{{% for group in view.groups %}}{{% for row in group.rows %}} {{{{ row.title }}}}{{% endfor %}}{{% endfor %}}"
{extra}
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

/// A GraphQL page of `o/open`, seen as `visibility`, with one open pull
/// request titled `title`, last acted on at `updated`.
fn page(title: &str, visibility: &str, updated: &str) -> String {
    json!({ "data": { "repository": { "visibility": visibility, "pullRequests": { "nodes": [ {
        "number": 1, "title": title, "url": "https://github.com/o/open/pull/1",
        "isDraft": false, "createdAt": "2026-10-20T10:00:00Z", "updatedAt": updated,
        "author": { "__typename": "User", "login": "ana" },
        "reviewDecision": "REVIEW_REQUIRED",
        "reviewRequests": { "totalCount": 1, "nodes": [ { "requestedReviewer": { "login": "bo" } } ] }
    } ] } } } })
    .to_string()
}

fn round(config: &Config, store: &Failing, script: &Script, at: Timestamp) -> Vec<Swept> {
    round_within(config, store, script, (at, None))
}

/// A round at `at`, its outbound requests bounded to `most` where it says.
fn round_within(
    config: &Config,
    store: &Failing,
    script: &Script,
    (at, most): (Timestamp, Option<usize>),
) -> Vec<Swept> {
    let secrets = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "hunter2")
        .with("HERALD_SLACK_WEBHOOK", HOOK)
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

fn hours(count: i64) -> Timestamp {
    NOON.checked_add(SignedDuration::from_hours(count)).unwrap()
}

/// Each announcement's identity, and what every route did with it.
fn announced(swept: &Swept) -> Vec<(String, Vec<(String, bool)>)> {
    let Sweep::Reported { announced, .. } = &swept.outcome else {
        panic!("{swept:?}");
    };
    announced
        .iter()
        .map(|fresh| {
            (
                fresh.event.identity().as_str().to_owned(),
                fresh
                    .announced
                    .iter()
                    .map(|route| {
                        (
                            route.route.as_str().to_owned(),
                            matches!(route.outcome, Outcome::Sent { .. }),
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

const BOARD: &str = r#"[boards.live]
view = "open"
every = "1h"
at = "00:00"
destination = "discord"
channel = "1566200000000000100"
template = "t"
"#;

#[test]
fn a_board_posts_and_pins_once_then_each_window_edits_its_post_where_it_changed() {
    let config = config(BOARD);
    let store = healthy();
    let first = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("Change", "PUBLIC", "2026-10-30T11:00:00Z"),
        ),
        POSTED,
        PINNED,
        WHO,
        NO_PINS,
    ]);
    let swept = round(&config, &store, &first, NOON);
    assert_eq!(
        announced(&swept[0]),
        [("report/live".to_owned(), vec![("live".to_owned(), true)])]
    );
    let unchanged = Script::answering(&[(
        GRAPHQL,
        200,
        &page("Change", "PUBLIC", "2026-10-30T11:00:00Z"),
    )]);
    let swept = round(&config, &store, &unchanged, hours(1));
    assert_eq!(
        announced(&swept[0]),
        [("report/live".to_owned(), vec![("live".to_owned(), false)])]
    );
    assert_eq!(unchanged.seen().len(), 1);
    let changed = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("Renamed", "PUBLIC", "2026-10-30T11:00:00Z"),
        ),
        EDITED,
    ]);
    let swept = round(&config, &store, &changed, hours(2));
    assert_eq!(
        announced(&swept[0]),
        [("report/live".to_owned(), vec![("live".to_owned(), true)])]
    );
    let edit = String::from_utf8(changed.bodies()[1].clone()).unwrap();
    assert!(edit.contains("Renamed"), "{edit}");
    assert!(
        changed.seen()[1].starts_with("PATCH "),
        "{:?}",
        changed.seen()
    );
}

#[test]
fn a_living_route_that_cannot_edit_posts_each_window_anew_beside_one_that_edits() {
    let config = config(&format!(
        "{BOARD}\n[boards.hooked]\nview = \"open\"\nevery = \"1h\"\nat = \"00:00\"\ndestination = \"slack\"\ntemplate = \"t\"\n"
    ));
    let store = healthy();
    let script = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("Change", "PUBLIC", "2026-10-30T11:00:00Z"),
        ),
        ("«secret:HERALD_SLACK_WEBHOOK»", 200, "ok"),
        POSTED,
        PINNED,
        WHO,
        NO_PINS,
    ]);
    let swept = round(&config, &store, &script, NOON);
    assert_eq!(swept[0].source, SourceName::new("hooked"));
    assert_eq!(
        announced(&swept[0]),
        [(
            "report/hooked/2026-10-31T12:00:00Z".to_owned(),
            vec![("hooked".to_owned(), true)]
        )]
    );
    assert_eq!(
        announced(&swept[1]),
        [("report/live".to_owned(), vec![("live".to_owned(), true)])]
    );
    assert_eq!(script.seen().len(), 6, "one read for both boards");
}

const RULE: &str = r#"[rules.waiting]
view = "open"
stages = ["1d", "3d"]
every = "1h"
at = "00:00"
[[routes]]
id = "nudges"
sources = ["waiting"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:nudge"
private = true
"#;

#[test]
fn a_rule_nudges_once_a_stage_and_spell_and_again_at_its_next_stage_or_spell() {
    let config = config(RULE);
    let store = healthy();
    let read = |updated: &str| (GRAPHQL, 200, page("Change", "PUBLIC", updated));
    let (graphql, status, body) = read("2026-10-30T11:00:00Z");
    let first = Script::answering(&[(graphql, status, &body), POSTED]);
    let swept = round(&config, &store, &first, NOON);
    assert_eq!(
        announced(&swept[0]),
        [(
            "rule/waiting/github.com/o/open/pull/1/2026-10-30T11:00:00Z/1d".to_owned(),
            vec![("nudges".to_owned(), true)]
        )]
    );
    let nudge = String::from_utf8(first.bodies()[1].clone()).unwrap();
    assert!(
        nudge.contains("o/open#1 has waited 1d") && nudge.contains("waiting on bo"),
        "{nudge}"
    );
    let again = Script::answering(&[(graphql, status, &body)]);
    let swept = round(&config, &store, &again, hours(1));
    assert_eq!(
        announced(&swept[0]),
        [(
            "rule/waiting/github.com/o/open/pull/1/2026-10-30T11:00:00Z/1d".to_owned(),
            vec![("nudges".to_owned(), false)]
        )]
    );
    assert_eq!(round(&config, &store, &Script::default(), hours(1)), []);
    let later = Script::answering(&[(graphql, status, &body), POSTED]);
    let swept = round(&config, &store, &later, hours(48));
    assert_eq!(
        announced(&swept[0])[0].0,
        "rule/waiting/github.com/o/open/pull/1/2026-10-30T11:00:00Z/3d"
    );
    let (graphql, status, body) = read("2026-11-02T11:00:00Z");
    let spell = Script::answering(&[(graphql, status, &body)]);
    let swept = round(&config, &store, &spell, hours(49));
    assert_eq!(announced(&swept[0]), []);
}

#[test]
fn a_private_pull_request_is_nudged_only_to_private_routes() {
    let config = config(&format!(
        "{RULE}[[routes]]\nid = \"public\"\nsources = [\"waiting\"]\ndestination = \"slack\"\ntemplate = \"herald:nudge\"\nprivate = false\n"
    ));
    let script = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("Change", "PRIVATE", "2026-10-30T11:00:00Z"),
        ),
        POSTED,
    ]);
    let swept = round(&config, &healthy(), &script, NOON);
    assert_eq!(announced(&swept[0])[0].1, [("nudges".to_owned(), true)]);
}

#[test]
fn a_rule_whose_view_cannot_be_read_or_whose_nudge_makes_no_event_stops_until_the_next_round() {
    let config = config(RULE);
    let store = healthy();
    let script = Script::answering(&[(GRAPHQL, 502, "")]);
    let swept = round(&config, &store, &script, NOON);
    assert!(
        matches!(&swept[0].outcome, Sweep::Stopped(Stop::Source(_))),
        "{swept:?}"
    );
    let untitled =
        Script::answering(&[(GRAPHQL, 200, &page(" ", "PUBLIC", "2026-10-30T11:00:00Z"))]);
    let swept = round(&config, &store, &untitled, NOON);
    assert!(
        matches!(&swept[0].outcome, Sweep::Stopped(Stop::Source(_))),
        "{swept:?}"
    );
    let failing = Failing {
        fault: crate::harness::Fault::Cursors,
        ..healthy()
    };
    let swept = round(&config, &failing, &Script::default(), NOON);
    assert!(
        matches!(&swept[0].outcome, Sweep::Stopped(Stop::Store(_))),
        "{swept:?}"
    );
    let stopped = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("Change", "PUBLIC", "2026-10-30T11:00:00Z"),
        ),
        ("/channels/1566200000000000100/messages", 500, ""),
    ]);
    let swept = round(&config, &store, &stopped, NOON);
    let Sweep::Reported { cursor, .. } = &swept[0].outcome else {
        panic!("{swept:?}");
    };
    assert_eq!(*cursor, Remembered::Kept);
}

const PLANS: &str = r#"[views.kept]
sources = ["open"]
mode = "kept"
reconcile = "1d"
[rules.flaky]
view = "kept"
flaky = { rerun = true }
every = "1h"
at = "00:00"
[[routes]]
id = "plans"
sources = ["flaky"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:rerun"
private = true
[[routes]]
id = "public"
sources = ["flaky"]
destination = "slack"
template = "herald:rerun"
private = false
"#;

/// `store` keeping `o/open#1` titled `title`, whose check `test` fails on
/// its head `h1` and failed then passed on it an hour before noon, listed
/// at noon.
pub(crate) fn flaky(store: &Failing, title: &str) {
    flaky_on(store, ("open", "github.com", "o/open"), title);
}

/// [`flaky`], its repository seen as `visibility`.
pub(crate) fn flaky_seen(store: &Failing, title: &str, visibility: herald_core::Visibility) {
    use herald_core::{Checks, Reconciled};
    use herald_ports::Pulls as _;
    let failed = (Checks::Failed(vec!["test".to_owned()]), visibility);
    keep_seen(store, ("github.com", "o/open"), (1, title), (failed, None));
    let mark = Reconciled {
        repository: Some(key("github.com")),
        at: Some(NOON),
        ..Reconciled::default()
    };
    pollster::block_on(store.put_reconciled(&SourceName::new("open"), &mark, None)).unwrap();
}

/// `store` keeping the pull request 1 of `repository` at `host` titled
/// `title`, read by the source `source`, whose check `test` fails on its
/// head `h1` and failed then passed on it an hour before noon, listed at
/// noon.
pub(crate) fn flaky_on(
    store: &Failing,
    (source, host, repository): (&str, &str, &str),
    title: &str,
) {
    use herald_core::{Checks, Reconciled};
    use herald_ports::Pulls as _;
    let failed = Checks::Failed(vec!["test".to_owned()]);
    keep(store, (host, repository), (1, title), failed);
    let mark = Reconciled {
        repository: Some(key(host)),
        at: Some(NOON),
        ..Reconciled::default()
    };
    pollster::block_on(store.put_reconciled(&SourceName::new(source), &mark, None)).unwrap();
}

fn key(host: &str) -> herald_core::RepositoryKey {
    herald_core::RepositoryKey {
        host: host.to_owned(),
        id: "1".to_owned(),
    }
}

/// `store` keeping `<repository>#<number>` at `host` titled `title` on its
/// head `h1` with `checks`, its check `test` having failed then passed on
/// it an hour before noon.
fn keep(
    store: &Failing,
    (host, repository): (&str, &str),
    (number, title): (u64, &str),
    checks: herald_core::Checks,
) {
    let seen = (checks, herald_core::Visibility::Public);
    keep_seen(store, (host, repository), (number, title), (seen, None));
}

/// [`keep`], its repository seen as `visibility`, its merge queue entry
/// `queue`.
fn keep_seen(
    store: &Failing,
    (host, repository): (&str, &str),
    (number, title): (u64, &str),
    ((checks, visibility), queue): (
        (herald_core::Checks, herald_core::Visibility),
        Option<herald_core::QueueEntry>,
    ),
) {
    let pull = seen_pull(
        (host, repository),
        (number, title),
        ((checks, visibility), queue),
    );
    keep_pull(store, host, pull);
}

/// The pull request `number` of `repository` at `host` titled `title`,
/// its checks and visibility `seen`, in the merge queue as `queue`, whose
/// check `test` failed then passed on its head `h1` an hour before noon.
fn seen_pull(
    (host, repository): (&str, &str),
    (number, title): (u64, &str),
    ((checks, visibility), queue): (
        (herald_core::Checks, herald_core::Visibility),
        Option<herald_core::QueueEntry>,
    ),
) -> herald_core::Pull {
    use herald_core::Flip;
    herald_core::Pull {
        project: "open".to_owned(),
        repository: repository.to_owned(),
        visibility,
        number,
        title: title.to_owned(),
        url: format!("https://{host}/{repository}/pull/{number}"),
        author: Some("ana".to_owned()),
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: hours(-48),
        updated: hours(-1),
        review: herald_core::Review::Approved,
        reviewers: Vec::new(),
        checks,
        mergeable: herald_core::Mergeable::Clean,
        queue,
        forge: None,
        head: Some("h1".to_owned()),
        unread: false,
        base: None,
        files: None,
        areas: Vec::new(),
        head_branch: None,
        milestone: None,
        flips: vec![Flip {
            check: "test".to_owned(),
            head: "h1".to_owned(),
            at: hours(-1),
        }],
    }
}

fn keep_pull(store: &Failing, host: &str, pull: herald_core::Pull) {
    use herald_core::{KeptPull, PullKey};
    use herald_ports::Pulls as _;
    let pulled = PullKey {
        repository: key(host),
        number: pull.number,
    };
    let record = KeptPull::read(None, pulled, pull, NOON);
    pollster::block_on(store.put_pull(&record, None)).unwrap();
}

#[test]
fn a_rule_plans_a_re_run_of_a_flaky_check_once_a_run_and_head_to_its_private_routes_alone() {
    let config = config(PLANS);
    let store = healthy();
    flaky(&store, "Change");
    let runs = json!({ "workflow_runs": [
        { "id": 42, "html_url": "https://github.com/o/open/actions/runs/42" },
        { "id": 43, "html_url": "https://github.com/o/open/actions/runs/43" }
    ] })
    .to_string();
    let jobs = |name: &str| {
        json!({ "jobs": [{ "id": 420, "name": name, "conclusion": "failure" }] }).to_string()
    };
    let (failing, other) = (jobs("test"), jobs("lint"));
    let script = Script::answering(&[
        (
            "/repos/o/open/actions/runs?head_sha=h1&status=failure&per_page=5",
            200,
            &runs,
        ),
        (
            "/repos/o/open/actions/runs/42/jobs?filter=latest&per_page=100",
            200,
            &failing,
        ),
        (
            "/repos/o/open/actions/runs/43/jobs?filter=latest&per_page=100",
            200,
            &other,
        ),
        POSTED,
    ]);
    let swept = round(&config, &store, &script, NOON);
    assert_eq!(
        announced(&swept[0]),
        [(
            "rerun/flaky/github.com/o/open/pull/1/test/h1/42".to_owned(),
            vec![("plans".to_owned(), true)]
        )]
    );
    let plan = String::from_utf8(script.bodies()[3].clone())
        .unwrap()
        .replace('\\', "");
    assert!(
        plan.contains("o/open#1: test looks flaky")
            && plan.contains("POST https://api.github.com/repos/o/open/actions/jobs/420/rerun"),
        "{plan}"
    );
    let again = Script::answering(&[
        (
            "/repos/o/open/actions/runs?head_sha=h1&status=failure&per_page=5",
            200,
            &runs,
        ),
        (
            "/repos/o/open/actions/runs/42/jobs?filter=latest&per_page=100",
            200,
            &failing,
        ),
        (
            "/repos/o/open/actions/runs/43/jobs?filter=latest&per_page=100",
            200,
            &other,
        ),
    ]);
    let swept = round(&config, &store, &again, hours(1));
    assert_eq!(announced(&swept[0])[0].1, [("plans".to_owned(), false)]);
}

#[test]
fn a_flaky_check_no_failed_run_ran_is_planned_with_the_reason_and_a_passing_pull_request_is_not() {
    let config = config(PLANS);
    let store = healthy();
    flaky(&store, "Change");
    keep(
        &store,
        ("github.com", "o/open"),
        (2, "Change"),
        herald_core::Checks::Passed,
    );
    let script = Script::answering(&[
        (
            "/repos/o/open/actions/runs?head_sha=h1&status=failure&per_page=5",
            200,
            r#"{"workflow_runs":[]}"#,
        ),
        POSTED,
    ]);
    let swept = round(&config, &store, &script, NOON);
    assert_eq!(
        announced(&swept[0]),
        [(
            "rerun/flaky/github.com/o/open/pull/1/test/h1/none".to_owned(),
            vec![("plans".to_owned(), true)]
        )]
    );
    let plan = String::from_utf8(script.bodies().last().unwrap().clone())
        .unwrap()
        .replace('\\', "");
    assert!(plan.contains("herald has nothing to re-run"), "{plan}");
    assert_eq!(script.seen().len(), 2);
}

#[test]
fn a_plan_that_makes_no_event_stops_the_rule_until_the_next_round() {
    let config = config(PLANS);
    let store = healthy();
    flaky(&store, " ");
    let script = Script::answering(&[(
        "/repos/o/open/actions/runs?head_sha=h1&status=failure&per_page=5",
        200,
        r#"{"workflow_runs":[]}"#,
    )]);
    let swept = round(&config, &store, &script, NOON);
    let Sweep::Stopped(Stop::Source(reason)) = &swept[0].outcome else {
        panic!("{swept:?}");
    };
    assert!(reason.contains("no title"), "{reason}");
    assert_eq!(script.seen().len(), 1);
}

#[test]
fn a_rule_whose_runs_cannot_be_read_stops_until_the_next_round() {
    let config = config(PLANS);
    let store = healthy();
    flaky(&store, "Change");
    let script = Script::answering(&[(
        "/repos/o/open/actions/runs?head_sha=h1&status=failure&per_page=5",
        502,
        "",
    )]);
    let swept = round(&config, &store, &script, NOON);
    assert!(matches!(swept[0].outcome, Sweep::Stopped(_)), "{swept:?}");
}

#[test]
fn a_plan_charges_each_request_it_makes_and_stops_where_the_budget_ends() {
    let config = config(PLANS);
    let store = healthy();
    flaky(&store, "Change");
    let runs = json!({ "workflow_runs": [
        { "id": 41, "html_url": "https://github.com/o/open/actions/runs/41" },
        { "id": 42, "html_url": "https://github.com/o/open/actions/runs/42" },
        { "id": 43, "html_url": "https://github.com/o/open/actions/runs/43" }
    ] })
    .to_string();
    let jobs =
        json!({ "jobs": [{ "id": 421, "name": "lint", "conclusion": "failure" }] }).to_string();
    let script = Script::answering(&[
        (
            "/repos/o/open/actions/runs?head_sha=h1&status=failure&per_page=5",
            200,
            &runs,
        ),
        (
            "/repos/o/open/actions/runs/41/jobs?filter=latest&per_page=100",
            200,
            &jobs,
        ),
        (
            "/repos/o/open/actions/runs/42/jobs?filter=latest&per_page=100",
            200,
            &jobs,
        ),
        (
            "/repos/o/open/actions/runs/43/jobs?filter=latest&per_page=100",
            200,
            &jobs,
        ),
    ]);
    let swept = round_within(&config, &store, &script, (NOON, Some(2)));
    let Sweep::Stopped(Stop::Limit(reason)) = &swept[0].outcome else {
        panic!("{swept:?}");
    };
    assert!(reason.contains("2 of the 2 outbound requests"), "{reason}");
    assert_eq!(script.seen().len(), 2);
}

const DEQUEUES: &str = r#"[views.kept]
sources = ["open"]
mode = "kept"
reconcile = "1d"
[rules.dequeued]
view = "kept"
dequeued = true
every = "1h"
at = "00:00"
[[routes]]
id = "alerts"
sources = ["dequeued"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:dequeued"
private = true
[[routes]]
id = "public"
sources = ["dequeued"]
destination = "slack"
template = "herald:dequeued"
private = false
"#;

#[test]
fn a_rule_alerts_once_on_a_private_pull_request_that_left_the_queue_for_a_failure_to_its_private_routes_alone()
 {
    use herald_core::{Checks, Dequeued, Left, QueueEntry, Visibility};
    let config = config(DEQUEUES);
    let store = healthy();
    flaky_seen(&store, "Change", Visibility::Private);
    for (number, reason) in [(2, Dequeued::ChecksFailed), (3, Dequeued::Removed)] {
        let left = Left {
            at: hours(-1),
            reason,
        };
        let queue = Some(QueueEntry::leaving(None, left));
        let seen = (Checks::Passed, Visibility::Private);
        keep_seen(
            &store,
            ("github.com", "o/open"),
            (number, "Change"),
            (seen, queue),
        );
    }
    let script = Script::answering(&[POSTED]);
    let swept = round(&config, &store, &script, NOON);
    assert_eq!(
        announced(&swept[0]),
        [(
            format!(
                "rule/dequeued/github.com/o/open/pull/2/dequeued/{}",
                hours(-1)
            ),
            vec![("alerts".to_owned(), true)]
        )]
    );
    let alert = String::from_utf8(script.bodies()[0].clone())
        .unwrap()
        .replace('\\', "");
    assert!(
        alert.contains("o/open#2 left the merge queue") && alert.contains("checks-failed"),
        "{alert}"
    );
    let again = Script::answering(&[]);
    let swept = round(&config, &store, &again, hours(1));
    assert_eq!(announced(&swept[0])[0].1, [("alerts".to_owned(), false)]);
}

const READINESS: &str = r#"[views.ship]
sources = ["open"]
mode = "kept"
reconcile = "1d"
release = { milestone = "v2.0", blocked_by = ["failed"], header = false }
[rules.ship]
view = "ship"
release = { ready = true }
every = "1h"
at = "00:00"
[[routes]]
id = "alerts"
sources = ["ship"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:readiness"
private = true
[[routes]]
id = "public"
sources = ["ship"]
destination = "slack"
template = "herald:readiness"
private = false
"#;

#[test]
fn a_release_that_turns_ready_is_told_to_each_audience_once_and_only_as_its_board_tells_it() {
    use herald_core::{Checks, Cursor, Identity, ReleaseVerdict, Visibility};
    use herald_ports::Pulls as _;
    use herald_ports::{Cursors as _, Store as _};
    let store = healthy();
    let mark = herald_core::Reconciled {
        repository: Some(key("github.com")),
        at: Some(NOON),
        ..herald_core::Reconciled::default()
    };
    pollster::block_on(store.put_reconciled(&SourceName::new("open"), &mark, None)).unwrap();
    for (number, visibility) in [(1, Visibility::Public), (2, Visibility::Private)] {
        let seen = ((Checks::Passed, visibility), None);
        let mut pull = seen_pull(("github.com", "o/open"), (number, "Change"), seen);
        pull.milestone = Some("v2.0".to_owned());
        keep_pull(&store, "github.com", pull);
    }
    let told = Cursor {
        seen: vec![Identity::new("rule/ship/earlier")],
        verdicts: [
            (Visibility::Private, ReleaseVerdict::Held),
            (Visibility::Public, ReleaseVerdict::Held),
        ]
        .into(),
        ..Cursor::default()
    };
    let cursors = store.cursors();
    pollster::block_on(cursors.put_cursor(&SourceName::new("ship"), &told, None)).unwrap();
    for (withheld, public) in [("unknown", false), ("shown", true)] {
        let config = config(&READINESS.replace(
            "blocked_by = [\"failed\"]",
            &format!("blocked_by = [\"failed\"], withheld = \"{withheld}\""),
        ));
        let slack = ("«secret:HERALD_SLACK_WEBHOOK»", 200, "ok");
        let script = Script::answering(&[if public { slack } else { POSTED }]);
        let swept = round(&config, &store, &script, NOON);
        let mut wanted = vec![(
            "rule/ship/release/private/ready/rule/ship/earlier".to_owned(),
            vec![("alerts".to_owned(), withheld == "unknown")],
        )];
        if public {
            wanted.push((
                "rule/ship/release/public/ready/rule/ship/earlier".to_owned(),
                vec![("public".to_owned(), true)],
            ));
        }
        assert_eq!(announced(&swept[0]), wanted, "{withheld}");
        let posted = script
            .bodies()
            .iter()
            .map(|body| String::from_utf8_lossy(body).into_owned())
            .collect::<Vec<_>>();
        assert!(
            posted.iter().all(|body| !body.contains("pull/2")),
            "{posted:?}"
        );
        let kept = pollster::block_on(cursors.cursor(&SourceName::new("ship")))
            .unwrap()
            .unwrap();
        let public_verdict = if public {
            ReleaseVerdict::Ready
        } else {
            ReleaseVerdict::Unknown
        };
        assert_eq!(
            kept.value.verdicts,
            [
                (Visibility::Private, ReleaseVerdict::Ready),
                (Visibility::Public, public_verdict)
            ]
            .into()
        );
        if public {
            continue;
        }
        let mut rewound = kept.value.clone();
        rewound.seen = told.seen.clone();
        rewound.verdicts = told.verdicts.clone();
        pollster::block_on(cursors.put_cursor(
            &SourceName::new("ship"),
            &rewound,
            Some(kept.version),
        ))
        .unwrap();
    }
    let config = config(READINESS);
    let again = Script::answering(&[]);
    let swept = round(&config, &store, &again, hours(1));
    assert!(announced(&swept[0]).is_empty(), "{swept:?}");
}
