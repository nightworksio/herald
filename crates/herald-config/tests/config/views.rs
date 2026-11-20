//! `[views]` and `[reports]`: the views of open pull requests, the built-in
//! `queue` among them, and the reports routes name among their sources,
//! every mistake in them named by key.

use herald_config::{BUILT_IN_VIEW, Config, QUEUE_TEMPLATE, load};
use herald_core::{Bots, Deciding, GroupBy, Kind, Rules, SourceName, Turn, View};
use herald_kinds::Most;
use jiff::SignedDuration;
use jiff::civil::{Time, Weekday};

const EXAMPLE: &str = r#"version = 1

[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[sources.herald]
kind = "forgejo"
repository = "nightworksio/herald"
api = "https://codeberg.org/api/v1"

[sources.org]
kind = "github"
owner = "nightworksio"

[[routes]]
id = "queue"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:queue"
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

#[test]
fn the_built_in_view_reads_every_source_naming_a_repository_or_an_owner() {
    let config = loaded(&with("[reports.daily]\n"));
    let queue = &config.views[BUILT_IN_VIEW];
    assert_eq!(queue.view, View::default());
    assert_eq!(
        queue.sources,
        [
            SourceName::new("gate"),
            SourceName::new("herald"),
            SourceName::new("org")
        ]
    );
    assert!(!queue.stated);
    assert_eq!(queue.most, Most::default());
    assert_eq!(
        (queue.mode, queue.reconcile),
        (herald_config::ViewMode::Auto, None)
    );
    let daily = &config.reports["daily"];
    assert_eq!(daily.view, BUILT_IN_VIEW);
    assert_eq!(daily.schedule, herald_core::ReportSchedule::default());
    let route = &config.routes[0].route;
    assert_eq!(route.filter.sources, [SourceName::new("daily")]);
    assert_eq!(
        route
            .template
            .as_ref()
            .map(herald_core::TemplateName::as_str),
        Some(QUEUE_TEMPLATE)
    );
    assert!(route.filter.kinds.is_empty() || route.filter.kinds.contains(&Kind::REPORT));
}

#[test]
fn a_view_reads_every_key_it_writes() {
    let config = loaded(&with(
        r#"[views.mine]
sources = ["herald"]
states = ["waiting-on-review", "waiting-on-merge"]
bots = "exclude"
repositories = ["nightworksio/*"]
authors = ["wessel"]
labels = ["needs-*"]
stale = "3d"
stale_only = true
group_by = "repository"
deciding = { labels = [{ label = "ready", state = "waiting-on-merge" }] }
rules = { draft = false, approval = false }
bumps = ["chore: bump {package} to {to}"]
most_open = 50
most_pulls = 10
mode = "kept"
reconcile = "15m"
listed_visibility = true
conflicts = true
broken_at = 5
flaky = { window = "3d", least = 2 }
overviews = ["capacity"]
capacity_from = "members"
capacity_repositories = 5

[views.forge]
deciding = "forge"
mode = "live"
group_by = "author"
bots = "only"

[views.everyone]
deciding = "herald"
group_by = "nothing"
bots = "include"

[views.batched]
group_by = "turn"
bots = "batch"

[reports.daily]
view = "mine"
every = "12h"
at = "08:30"
days = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
zone = "Europe/Amsterdam"
"#,
    ));
    let mine = &config.views["mine"];
    assert_eq!(mine.sources, [SourceName::new("herald")]);
    assert_eq!(
        mine.view.states,
        [Turn::WaitingOnReview, Turn::WaitingOnMerge]
    );
    assert_eq!(mine.view.bots, Bots::Exclude);
    assert!(mine.view.repositories[0].matches("nightworksio/herald"));
    assert!(mine.view.authors[0].matches("wessel"));
    assert!(mine.view.labels[0].matches("needs-review"));
    assert_eq!(mine.view.stale, SignedDuration::from_hours(72));
    assert_eq!(mine.mode, herald_config::ViewMode::Kept);
    assert_eq!(mine.reconcile, Some(SignedDuration::from_mins(15)));
    assert!(mine.stated);
    assert!(!config.views["forge"].stated);
    assert!(mine.conflicts);
    assert_eq!(
        mine.capacity,
        Some(herald_core::Capacity::Members { most: 5 })
    );
    assert_eq!(config.views["forge"].capacity, None);
    assert_eq!(
        mine.view.triage,
        herald_core::Triage {
            broken_at: 5,
            flaky_window: SignedDuration::from_hours(72),
            flaky_least: 2
        }
    );
    assert_eq!(
        config.views["forge"].view.triage,
        herald_core::Triage::default()
    );
    assert!(!config.views["forge"].conflicts);
    assert_eq!(config.views["forge"].mode, herald_config::ViewMode::Live);
    assert_eq!(config.views["everyone"].mode, herald_config::ViewMode::Auto);
    assert_eq!(config.views["everyone"].reconcile, None);
    assert!(mine.view.stale_only);
    assert_eq!(mine.view.group_by, GroupBy::Repository);
    assert_eq!(
        mine.view.deciding,
        Deciding::Labels(vec![("ready".to_owned(), Turn::WaitingOnMerge)])
    );
    assert_eq!(
        mine.view.rules,
        Rules {
            draft: false,
            approval: false,
            ..Rules::default()
        }
    );
    assert_eq!(mine.view.bumps[0].as_str(), "chore: bump {package} to {to}");
    assert_eq!(
        mine.most,
        Most {
            open: 50,
            pulls: 10
        }
    );
    let forge = &config.views["forge"];
    assert_eq!(
        (
            forge.view.deciding.clone(),
            forge.view.group_by,
            forge.view.bots
        ),
        (Deciding::Forge, GroupBy::Author, Bots::Only)
    );
    let everyone = &config.views["everyone"];
    assert_eq!(
        (
            everyone.view.deciding.clone(),
            everyone.view.group_by,
            everyone.view.bots
        ),
        (Deciding::Herald, GroupBy::Nothing, Bots::Include)
    );
    let batched = &config.views["batched"];
    assert_eq!(
        (batched.view.group_by, batched.view.bots),
        (GroupBy::Turn, Bots::Batch)
    );
    let daily = &config.reports["daily"];
    assert_eq!(daily.view, "mine");
    assert_eq!(daily.schedule.every, SignedDuration::from_hours(12));
    assert_eq!(daily.schedule.at, Time::constant(8, 30, 0, 0));
    assert_eq!(
        daily.schedule.days,
        [
            Weekday::Monday,
            Weekday::Tuesday,
            Weekday::Wednesday,
            Weekday::Thursday,
            Weekday::Friday,
            Weekday::Saturday,
            Weekday::Sunday
        ]
    );
    assert_eq!(daily.schedule.zone.iana_name(), Some("Europe/Amsterdam"));
}

#[test]
fn a_view_named_queue_replaces_the_built_in_one() {
    let config = loaded(&with(
        "[views.queue]\nsources = [\"gate\"]\n\n[reports.daily]\n",
    ));
    assert_eq!(
        config.views[BUILT_IN_VIEW].sources,
        [SourceName::new("gate")]
    );
}

#[test]
fn a_view_shows_the_capacity_of_the_repositories_it_shows_where_it_names_no_other() {
    let config = loaded(&with(
        "[views.ci]\noverviews = [\"capacity\"]\n\n[reports.daily]\n",
    ));
    assert_eq!(
        config.views["ci"].capacity,
        Some(herald_core::Capacity::Shown)
    );
    let members = loaded(&with(
        "[views.ci]\noverviews = [\"capacity\"]\ncapacity_from = \"members\"\n\n[reports.daily]\n",
    ));
    assert_eq!(
        members.views["ci"].capacity,
        Some(herald_core::Capacity::Members {
            most: herald_core::CAPACITY_REPOSITORIES
        })
    );
    let error = errors(&with(
        "[views.wide]\noverviews = [\"capacity\"]\ncapacity_repositories = 101\n\n[views.unshown]\ncapacity_from = \"members\"\n\n[views.odd]\noverviews = [\"weather\"]\n",
    ));
    for wanted in [
        "views.wide.capacity_repositories: a capacity overview reads from 1 to 100 repositories, such as capacity_repositories = 20",
        "views.unshown: capacity_from and capacity_repositories take effect with overviews = [\"capacity\"], which this view does not name",
        "views.odd",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
}

#[test]
fn every_mistake_in_a_view_or_report_is_named_by_its_key() {
    let error = errors(&with(
        r#"[sources.news]
kind = "feed"
url = "https://blog.example/feed.xml"

[views.mine]
sources = ["news", "nowhere"]
repositories = ["["]
authors = ["["]
labels = ["["]
bumps = ["no package"]

[reports.daily]
view = "missing"
every = "2d"
at = "9:00"
zone = "Mars/Olympus"

[reports.gate]
at = "25:00"

[reports.short]
every = "30s"

[reports.typed]
days = "mon"
"#,
    ));
    for wanted in [
        "views.mine.sources: \"news\" names no repository or owner on a forge, so it lists no pull requests a view reads; the sources a view reads are \"gate\", \"herald\", \"org\"",
        "views.mine.sources: \"nowhere\" is no source; the sources a view reads are \"gate\", \"herald\", \"org\"",
        "views.mine.repositories[0]:",
        "views.mine.authors[0]:",
        "views.mine.labels[0]:",
        "views.mine.bumps[0]: a bump pattern names the package as {package}",
        "reports.daily.view: no view is named \"missing\"; the views are \"mine\", \"queue\"",
        "reports.daily.every: a report's window lasts from \"1m\" to \"1d\"",
        "reports.daily.at: a report begins at a wall-clock time written \"HH:MM\", such as at = \"09:00\"",
        "reports.daily.zone: there is no time zone \"Mars/Olympus\"; name one as the IANA database does, such as zone = \"Europe/Amsterdam\", or \"UTC\"",
        "reports.gate: a source, app, report or rule is named \"gate\" too; a route names each alike, so give each its own name",
        "reports.gate.at: a report begins at a wall-clock time written",
        "reports.short.every: a report's window lasts from \"1m\" to \"1d\"",
        "reports.typed.days:",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
}

#[test]
fn a_report_is_its_route_s_source_and_no_app_or_peer_s_kind() {
    let error = errors(&with(
        "[reports.daily]\n\n[[routes]]\nid = \"other\"\nsources = [\"gate\"]\nkinds = [\"report\"]\ndestination = \"discord\"\nchannel = \"1566200000000000200\"\ntemplate = \"herald:queue\"\n",
    ));
    assert!(
        error.contains("no source the route names announces a \"report\" event"),
        "{error}"
    );
    let error = errors(&with(
        "[reports.daily]\n\n[templates.\"herald:queue\"]\ntitle = \"{{ view.missing.key }}\"\nbody = \"\"\n",
    ));
    assert!(
        error.contains("it does not render a sample report event"),
        "{error}"
    );
}
