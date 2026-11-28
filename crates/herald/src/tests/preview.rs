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
