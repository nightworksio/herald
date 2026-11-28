use std::collections::BTreeMap;

use super::world::*;
use crate::*;

/// A source announcing releases and issues to one route, and a daily
/// report of the queue to another.
const KINDS: &str = r#"version = 1
[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.cli]
kind = "github"
repository = "cli/cli"
signals = ["release", "issue"]
[reports.daily]
view = "queue"
[[routes]]
id = "everything"
sources = ["cli"]
destination = "discord"
channel = "1566200000000000100"
template = "shown"
[[routes]]
id = "board"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"
template = "board"
private = false
[templates.shown]
title = "{{ event.kind }}: {{ event.title }}"
body = "{{ event.body | length }} characters"
[templates.board]
title = "{{ view.name }} of {{ event.attributes.pulls }}"
body = "{% for group in view.groups %}{{ group.name }}{% endfor %}"
"#;

/// The lines of `out` that name a route and what it renders.
fn heads(out: &str) -> Vec<&str> {
    out.lines().filter(|line| !line.starts_with(' ')).collect()
}

fn previewed(config: &str, args: &[&str]) -> Run {
    let mut all = vec!["preview"];
    all.extend(args);
    herald(&setup(config), &World::default(), &all)
}

#[test]
fn preview_renders_each_route_against_a_sample_or_a_release_event() {
    let setup = setup(CONFIG);
    let world = World::default();
    assert_eq!(
        heads(&herald(&setup, &world, &["preview"]).out),
        [
            "releases to discord, sample release (plain):",
            "releases to discord, sample release (pre-release):",
            "releases to discord, sample release (long):",
            "releases to discord, sample release (no-body):",
        ]
    );
    let sampled = herald(&setup, &world, &["preview", "--sample", "plain"]);
    assert_eq!(
        (sampled.code, sampled.out.as_str()),
        (
            DONE,
            "releases to discord, sample release (plain):\n  title: 📦 sample-project v1.2.0\n  part 1:\n    ## Added\n    \n    - a sample change\n"
        )
    );
    let path = setup
        .directory
        .path()
        .join("event.json")
        .display()
        .to_string();
    let evented = herald(
        &setup,
        &world,
        &["preview", "--event", &path, "--route", "releases"],
    );
    assert!(
        evented
            .out
            .starts_with("releases to discord:\n  title: 📦 cli v2.103.3\n"),
        "{}",
        evented.out
    );
    let threaded = super::world::setup(&CONFIG.replace(
        "template = \"release\"\n",
        "template = \"release\"\nthread = { name = \"{{ event.attributes.version }}\" }\n",
    ));
    assert!(
        herald(&threaded, &world, &["preview"])
            .out
            .contains("\n  thread: v1.2.0\n")
    );
}

#[test]
fn preview_renders_every_sample_of_each_kind_each_route_takes_and_each_report_showing_a_sample_view()
 {
    let every = previewed(KINDS, &[]);
    assert_eq!(every.code, DONE);
    assert_eq!(
        heads(&every.out),
        [
            "everything to discord, sample issue (plain):",
            "everything to discord, sample issue (long):",
            "everything to discord, sample issue (no-body):",
            "everything to discord, sample release (plain):",
            "everything to discord, sample release (pre-release):",
            "everything to discord, sample release (long):",
            "everything to discord, sample release (no-body):",
            "board to discord, report daily (sample view):",
        ]
    );
    assert_eq!(
        heads(&previewed(KINDS, &["--kind", "issue"]).out),
        [
            "everything to discord, sample issue (plain):",
            "everything to discord, sample issue (long):",
            "everything to discord, sample issue (no-body):",
        ]
    );
    let plain = previewed(KINDS, &["--sample", "plain"]);
    assert_eq!(
        plain.out,
        "everything to discord, sample issue (plain):\n  title: issue: Sample project 1.2.0\n  part 1:\n    27 characters\neverything to discord, sample release (plain):\n  title: release: Sample project 1.2.0\n  part 1:\n    27 characters\nboard to discord, report daily (sample view):\n  title: queue of 2\n  part 1:\n    waiting\\-on\\-review\n"
    );
    let long = previewed(KINDS, &["--route", "everything", "--sample", "long"]);
    assert_eq!(
        long.out,
        "everything to discord, sample issue (long):\n  title: issue: Sample project 1.2.0\n  part 1:\n    35999 characters\neverything to discord, sample release (long):\n  title: release: Sample project 1.2.0\n  part 1:\n    35999 characters\n"
    );
    let bodiless = previewed(KINDS, &["--sample", "no-body"]);
    assert_eq!(
        bodiless.out,
        "everything to discord, sample issue (no-body):\n  title: issue: Sample project 1.2.0\n  part 1:\n    0 characters\neverything to discord, sample release (no-body):\n  title: release: Sample project 1.2.0\n  part 1:\n    0 characters\n"
    );
    let prerelease = previewed(KINDS, &["--sample", "pre-release"]);
    assert_eq!(
        prerelease.out,
        "everything to discord, sample release (pre-release):\n  title: release: Sample project 1.2.0\n  part 1:\n    27 characters\n"
    );
    for asked in [&["--report", "daily"][..], &["--kind", "report"]] {
        assert_eq!(
            previewed(KINDS, asked).out,
            "board to discord, report daily (sample view):\n  title: queue of 2\n  part 1:\n    waiting\\-on\\-review\n"
        );
    }
}

#[test]
fn preview_renders_each_grouping_route_against_every_sample_or_the_event_as_one_group() {
    let world = World::default();
    let grouping = setup(&CONFIG.replace(
        "template = \"release\"\n",
        "template = \"release\"\ngroup = { key = \"{{ event.attributes.version }}\" }\n",
    ).replace(
        "title = \"📦 {{ event.project }} {{ event.attributes.version }}\"",
        "title = \"{% if group %}{{ group.key }} of {{ events | length }}{% else %}alone{% endif %}\"",
    ));
    let sampled = herald(&grouping, &world, &["preview", "--group", "v9"]);
    assert_eq!(sampled.code, DONE, "{}", sampled.err);
    assert!(
        sampled
            .out
            .starts_with("releases to discord, samples as one group (4):\n  title: v9 of 4\n"),
        "{}",
        sampled.out
    );
    let one = herald(
        &grouping,
        &world,
        &["preview", "--group", "v9", "--sample", "pre-release"],
    );
    assert!(
        one.out
            .starts_with("releases to discord, samples as one group (1):\n  title: v9 of 1\n"),
        "{}",
        one.out
    );
    let path = grouping
        .directory
        .path()
        .join("event.json")
        .display()
        .to_string();
    let evented = herald(
        &grouping,
        &world,
        &["preview", "--group", "v9", "--event", &path],
    );
    assert!(
        evented
            .out
            .starts_with("releases to discord:\n  title: v9 of 1\n"),
        "{}",
        evented.out
    );
    assert!(
        herald(&grouping, &world, &["preview"])
            .out
            .starts_with("releases to discord, sample release (plain):\n  title: alone\n")
    );
    let ungrouped = herald(&setup(CONFIG), &world, &["preview", "--group", "v9"]);
    assert_eq!(
        (ungrouped.code, ungrouped.err.as_str()),
        (
            MISTAKEN,
            "nothing to preview: no route that groups takes events\n"
        )
    );
}

#[test]
fn preview_names_only_what_the_config_has_where_a_flag_names_nothing_of_it() {
    let typed = format!("{}{}", "hooks.slack.com/", "T0/B0/x");
    for (flag, said) in [
        (
            "--route",
            "--route names no route; the routes are everything, board",
        ),
        (
            "--kind",
            "--kind names no kind; the kinds are release, package-version, feed-entry, message, run, alert, pull-request, issue, milestone, report, nudge, rerun, dequeued, readiness",
        ),
        (
            "--sample",
            "--sample names no sample; the samples are plain, pre-release, long, no-body",
        ),
        (
            "--report",
            "--report names no report; the reports are daily",
        ),
    ] {
        let refused = previewed(KINDS, &[flag, &typed]);
        assert_eq!(
            (refused.code, refused.err.as_str(), refused.out.as_str()),
            (MISTAKEN, format!("{said}\n").as_str(), "")
        );
    }
    assert_eq!(
        previewed(CONFIG, &["--report", &typed]).err,
        "--report names no report, and the config has none\n"
    );
    for (asked, said) in [
        (&["--kind", "alert"][..], "no route takes \"alert\" events"),
        (
            &["--route", "board", "--kind", "issue", "--sample", "long"],
            "no route \"board\" takes \"issue\" events with a long sample",
        ),
        (
            &["--route", "everything", "--report", "daily"],
            "no route \"everything\" takes the report \"daily\"",
        ),
        (
            &["--kind", "report", "--sample", "long"],
            "no route takes \"report\" events with a long sample",
        ),
    ] {
        let unpreviewed = previewed(KINDS, asked);
        assert_eq!(
            (unpreviewed.code, unpreviewed.err),
            (MISTAKEN, format!("nothing to preview: {said}\n"))
        );
    }
    let setup = setup(KINDS);
    let event = setup.directory.path().join("event.json");
    let untaken = herald(
        &setup,
        &World::default(),
        &[
            "preview",
            "--route",
            "board",
            "--event",
            &event.display().to_string(),
        ],
    );
    assert_eq!(
        (untaken.code, untaken.err.as_str()),
        (
            MISTAKEN,
            "nothing to preview: no route \"board\" takes the event\n"
        )
    );
}

#[test]
fn preview_reads_no_secret_and_opens_no_connection() {
    let setup = setup(KINDS);
    let config = setup
        .directory
        .path()
        .join("herald.toml")
        .display()
        .to_string();
    let event = setup
        .directory
        .path()
        .join("event.json")
        .display()
        .to_string();
    let refused = |_: herald_ports::Base<'_>,
                   _: &herald_ports::Outbound|
     -> Result<Connection, String> { panic!("preview opened a connection") };
    for asked in [
        &[][..],
        &["--kind", "issue"],
        &["--report", "daily"],
        &["--sample", "long"],
        &["--event", &event],
        &["--format", "wire"],
        &["--format", "wire", "--event", &event],
        &["--format", "json"],
        &["--format", "json", "--report", "daily"],
    ] {
        let mut args = vec!["herald", "--config", &config, "preview"];
        args.extend(asked);
        let args: Vec<String> = args.into_iter().map(ToOwned::to_owned).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = on_tokio(crate::run(
            &args,
            &BTreeMap::new(),
            &Connector::new(&refused, &|_, _| panic!("preview waited on a connection")),
            &mut out,
            &mut err,
        ));
        let (out, err) = (
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        );
        assert_eq!((code, err.as_str()), (DONE, ""), "{asked:?}: {out}");
        assert!(!out.is_empty(), "{asked:?}");
    }
}

#[test]
fn preview_says_why_an_event_file_or_a_template_fails() {
    let world = World::default();
    let failing = setup(&CONFIG.replace(
        "title = \"📦 {{ event.project }} {{ event.attributes.version }}\"",
        "title = \"{% if event.project == 'cli' %}{{ nothing.here }}{% endif %}📦\"",
    ));
    let path = failing
        .directory
        .path()
        .join("event.json")
        .display()
        .to_string();
    let refused = herald(&failing, &world, &["preview", "--event", &path]);
    assert_eq!(refused.code, STOPPED);
    assert!(
        refused
            .out
            .starts_with("releases to discord: the templates cannot render this event:"),
        "{}",
        refused.out
    );
    let setup = setup(CONFIG);
    let missing = herald(
        &setup,
        &world,
        &["preview", "--event", "/nonexistent/event.json"],
    );
    assert_eq!(missing.code, MISTAKEN);
    assert!(
        missing
            .err
            .starts_with("/nonexistent/event.json could not be read")
    );
    std::fs::write(
        setup.directory.path().join("draft.json"),
        EVENT.replace("\"published\"", "\"created\""),
    )
    .unwrap();
    let draft = setup
        .directory
        .path()
        .join("draft.json")
        .display()
        .to_string();
    let unpublished = herald(&setup, &world, &["preview", "--event", &draft]);
    assert_eq!(
        (unpublished.code, unpublished.err),
        (MISTAKEN, format!("{draft} publishes no release\n"))
    );
    std::fs::write(setup.directory.path().join("garbled.json"), "{}").unwrap();
    let garbled = setup
        .directory
        .path()
        .join("garbled.json")
        .display()
        .to_string();
    assert_eq!(
        herald(&setup, &world, &["preview", "--event", &garbled]).code,
        MISTAKEN
    );
}

/// The test config, its route crossposting and keeping its pin in the
/// store, beside a Slack webhook's route and an ActivityPub actor's.
fn wired() -> String {
    format!(
        "{}{}",
        CONFIG.replace(
            "template = \"release\"\n",
            "template = \"release\"\ncrosspost = true\npin = \"store\"\n",
        ),
        r#"[destinations.slack]
kind = "slack"
webhook = { secret = "HERALD_SLACK_WEBHOOK" }
[[routes]]
id = "hooked"
sources = ["cli"]
destination = "slack"
template = "release"
[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }
[[routes]]
id = "followed"
sources = ["cli"]
destination = "fedi"
template = "release"
actor = "cli"
"#
    )
}

#[test]
fn preview_wire_prints_each_request_a_first_post_sends_with_each_secret_by_its_name() {
    let shown = previewed(&wired(), &["--format", "wire", "--sample", "plain"]);
    assert_eq!(
        (shown.code, shown.err.as_str()),
        (DONE, ""),
        "{}",
        shown.out
    );
    let lines: Vec<&str> = shown
        .out
        .lines()
        .filter(|line| !line.starts_with("    User-Agent: "))
        .collect();
    let channel = "https://discord.com/api/v10/channels/1566200000000000100/messages";
    let bot = "    Authorization: Bot «secret:HERALD_DISCORD_TOKEN»";
    let json = "    Content-Type: application/json; charset=utf-8";
    assert_eq!(
        lines,
        [
            "releases to discord, sample release (plain):",
            "  post:",
            &format!("    POST {channel}"),
            bot,
            "    Content-Type: application/json",
            "    ",
            lines[6],
            "  crosspost:",
            &format!("    POST {channel}/0/crosspost"),
            bot,
            "  pin:",
            &format!("    PUT {channel}/pins/0"),
            bot,
            "  unpin: no request",
            "hooked to slack, sample release (plain):",
            "  post:",
            "    POST «secret:HERALD_SLACK_WEBHOOK»",
            json,
            "    ",
            lines[19],
            "followed to fedi, sample release (plain): herald delivers it to each follower's inbox, and preview knows no follower, so it shows no request",
        ]
    );
    assert!(
        lines[6].starts_with("    {\"allowed_mentions\"")
            && lines[6].contains(r###""description":"## Added\n\n- a sample change""###),
        "{}",
        lines[6]
    );
    assert!(
        lines[19].starts_with("    {\"blocks\"") && lines[19].contains("a sample change"),
        "{}",
        lines[19]
    );
    assert!(!shown.out.contains("discord-token"), "{}", shown.out);
}

#[test]
fn preview_wire_prints_a_teams_card_posted_to_its_webhook_by_the_secret_s_name() {
    let config = format!(
        "{CONFIG}{}",
        r#"[destinations.teams]
kind = "teams"
webhook = { secret = "HERALD_TEAMS_WEBHOOK" }
[[routes]]
id = "carded"
sources = ["cli"]
destination = "teams"
template = "release"
"#
    );
    let shown = previewed(&config, &["--format", "wire", "--sample", "plain"]);
    assert_eq!(
        (shown.code, shown.err.as_str()),
        (DONE, ""),
        "{}",
        shown.out
    );
    let carded: Vec<&str> = shown
        .out
        .lines()
        .skip_while(|line| !line.starts_with("carded to teams"))
        .filter(|line| !line.starts_with("    User-Agent: "))
        .collect();
    assert_eq!(
        carded.get(..5).unwrap_or_else(|| panic!("{}", shown.out)),
        [
            "carded to teams, sample release (plain):",
            "  post:",
            "    POST «secret:HERALD_TEAMS_WEBHOOK»",
            "    Content-Type: application/json; charset=utf-8",
            "    ",
        ]
    );
    assert!(
        carded[5].starts_with(r#"    {"attachments":[{"content":{"$schema":"http://adaptivecards.io/schemas/adaptive-card.json","#)
            && carded[5].contains(r#""text":"**Added**\n\n- a sample change","type":"TextBlock","wrap":true"#)
            && carded[5].contains(r#""contentType":"application/vnd.microsoft.card.adaptive""#),
        "{}",
        carded[5]
    );
}

#[test]
fn preview_json_holds_what_each_route_renders_against_its_text_and_its_requests() {
    let shown = previewed(&wired(), &["--format", "json", "--sample", "plain"]);
    assert_eq!(
        (shown.code, shown.err.as_str()),
        (DONE, ""),
        "{}",
        shown.out
    );
    assert_eq!(shown.out.lines().count(), 1, "{}", shown.out);
    let all: serde_json::Value = serde_json::from_str(&shown.out).unwrap();
    let discord = &all[0];
    let post = &discord["steps"][0]["requests"][0];
    assert_eq!(
        (
            &discord["route"],
            &discord["destination"],
            &discord["against"],
            &discord["title"],
            &discord["thread"],
            &discord["parts"],
            &discord["unsent"],
        ),
        (
            &serde_json::json!("releases"),
            &serde_json::json!("discord"),
            &serde_json::json!({ "kind": "release", "sample": "plain" }),
            &serde_json::json!("📦 sample-project v1.2.0"),
            &serde_json::Value::Null,
            &serde_json::json!(["## Added\n\n- a sample change"]),
            &serde_json::Value::Null,
        )
    );
    assert_eq!(
        (
            &discord["steps"][0]["step"],
            &post["method"],
            &post["url"],
            &post["body"]["content"],
        ),
        (
            &serde_json::json!("post"),
            &serde_json::json!("POST"),
            &serde_json::json!("https://discord.com/api/v10/channels/1566200000000000100/messages"),
            &serde_json::json!(""),
        )
    );
    assert!(
        post["headers"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!([
                "Authorization",
                "Bot «secret:HERALD_DISCORD_TOKEN»"
            ])),
        "{post}"
    );
    let crossposted = &discord["steps"][1];
    assert_eq!(
        (&crossposted["step"], &crossposted["requests"][0]["body"]),
        (&serde_json::json!("crosspost"), &serde_json::Value::Null)
    );
    assert_eq!(
        discord["steps"][3],
        serde_json::json!({ "step": "unpin", "requests": [] })
    );
    assert_eq!(
        all[1]["steps"][0]["requests"][0]["url"],
        "«secret:HERALD_SLACK_WEBHOOK»"
    );
    assert_eq!(
        (&all[2]["steps"], &all[2]["unsent"]),
        (
            &serde_json::json!([]),
            &serde_json::json!(
                "herald delivers it to each follower's inbox, and preview knows no follower, so it shows no request"
            )
        )
    );
    let reported = previewed(KINDS, &["--format", "json", "--report", "daily"]);
    let report: serde_json::Value = serde_json::from_str(&reported.out).unwrap();
    assert_eq!(
        report[0]["against"],
        serde_json::json!({ "report": "daily" })
    );
}

#[test]
fn preview_json_and_wire_name_an_event_file_a_group_and_a_template_that_fails() {
    let world = World::default();
    let setup = setup(CONFIG);
    let path = setup
        .directory
        .path()
        .join("event.json")
        .display()
        .to_string();
    let evented = herald(
        &setup,
        &world,
        &["preview", "--format", "json", "--event", &path],
    );
    let event: serde_json::Value = serde_json::from_str(&evented.out).unwrap();
    assert_eq!(
        (&event[0]["against"], &event[0]["title"]),
        (
            &serde_json::Value::Null,
            &serde_json::json!("📦 cli v2.103.3")
        )
    );
    let grouping = super::world::setup(&CONFIG.replace(
        "template = \"release\"\n",
        "template = \"release\"\ngroup = { key = \"{{ event.attributes.version }}\" }\n",
    ));
    let grouped = herald(
        &grouping,
        &world,
        &["preview", "--format", "json", "--group", "v9"],
    );
    let group: serde_json::Value = serde_json::from_str(&grouped.out).unwrap();
    assert_eq!(group[0]["against"], serde_json::json!({ "group": 4 }));
    let failing = super::world::setup(&CONFIG.replace(
        "title = \"📦 {{ event.project }} {{ event.attributes.version }}\"",
        "title = \"{% if event.project == 'cli' %}{{ nothing.here }}{% endif %}📦\"",
    ));
    let wire = herald(
        &failing,
        &world,
        &["preview", "--format", "wire", "--event", &path],
    );
    assert_eq!(wire.code, STOPPED);
    assert!(
        wire.out
            .starts_with("releases to discord: the templates cannot render this event:"),
        "{}",
        wire.out
    );
    let json = herald(
        &failing,
        &world,
        &["preview", "--format", "json", "--event", &path],
    );
    let failed: serde_json::Value = serde_json::from_str(&json.out).unwrap();
    assert_eq!(json.code, STOPPED);
    assert_eq!(
        (
            &failed[0]["route"],
            failed[0]["error"].is_string(),
            &failed[0]["steps"]
        ),
        (
            &serde_json::json!("releases"),
            true,
            &serde_json::Value::Null
        )
    );
}

#[test]
fn a_request_as_json_holds_its_body_as_json_as_text_or_by_its_length_where_sealed() {
    use herald_ports::{Method, Request, SecretText};
    let request = |body: &str| {
        Request::new(Method::Post, SecretText::plain("https://example.org")).body(body)
    };
    assert_eq!(
        crate::preview::request_json(&request(r#"{"a":1}"#))["body"],
        serde_json::json!({ "a": 1 })
    );
    assert_eq!(
        crate::preview::request_json(&request("To: someone\r\n"))["body"],
        serde_json::json!("To: someone\r\n")
    );
    let sealed = Request::new(Method::Post, SecretText::plain("https://example.org"))
        .sealed_body("a secret");
    assert_eq!(
        crate::preview::request_json(&sealed),
        serde_json::json!({
            "method": "POST",
            "url": "https://example.org",
            "headers": [],
            "body": "«sealed: 8 bytes»",
        })
    );
}
