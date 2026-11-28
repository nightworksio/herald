use herald_config::{Config, Sample, load};
use herald_core::{Kind, RouteId};
use herald_flows::{Against, Previewed, Sampling, preview, preview_group, preview_samples};

use crate::harness::*;

#[test]
fn each_route_renders_the_event_as_its_destination_would_show_it() {
    let config = config("");
    let previewed = preview(&config, &event("v1"), None);
    assert_eq!(previewed.len(), 1);
    assert_eq!(previewed[0].route, RouteId::new("releases"));
    assert_eq!(previewed[0].destination.as_str(), "discord");
    let announcement = previewed[0].rendered.as_ref().unwrap();
    assert_eq!(announcement.title.plain(), "📦 php-mutation-gate v1");
    assert_eq!(announcement.thread.as_deref(), Some("v1"));
    assert_eq!(announcement.parts[0].body, "## Added");
    assert_eq!(
        preview(&config, &event("v1"), Some(&RouteId::new("elsewhere"))),
        []
    );
    assert_eq!(
        preview(&config, &event("v1"), Some(&RouteId::new("releases"))).len(),
        1
    );
}

#[test]
fn an_event_a_template_cannot_render_says_why() {
    let previewed = preview(&config(""), &event("boom"), None);
    let error = previewed[0].rendered.as_ref().unwrap_err();
    assert!(error.contains("undefined value"), "{error}");
}

#[test]
fn each_route_that_groups_renders_the_events_as_one_group_led_by_its_lead() {
    use crate::grouping::{grouped, release};
    let config = grouped("", "");
    let events = [release("cli", 2), release("core", 1)];
    let previewed = herald_flows::preview_group(&config, ("v9", &events), None);
    assert_eq!(previewed.len(), 1);
    assert_eq!(previewed[0].route, RouteId::new("train"));
    let announcement = previewed[0].rendered.as_ref().unwrap();
    assert_eq!(announcement.title.plain(), "v9 of 2");
    assert_eq!(announcement.project, "core");
    assert!(
        announcement.parts[0]
            .body
            .starts_with("core with notes and cli with notes"),
        "{}",
        announcement.parts[0].body
    );
    let only = Some(RouteId::new("train"));
    assert_eq!(
        herald_flows::preview_group(&config, ("v9", &events), only.as_ref()).len(),
        1
    );
    let elsewhere = Some(RouteId::new("elsewhere"));
    assert_eq!(
        herald_flows::preview_group(&config, ("v9", &events), elsewhere.as_ref()),
        []
    );
    assert_eq!(herald_flows::preview_group(&config, ("v9", &[]), None), []);
    assert_eq!(
        herald_flows::preview_group(&crate::harness::config(""), ("v9", &events), None),
        []
    );
    let alone = preview(&config, &events[0], None);
    assert_eq!(
        alone[0].rendered.as_ref().unwrap().title.plain(),
        "cli alone"
    );
}

/// A source announcing releases and issues to one route, and a daily
/// report of the queue to another.
const KINDS: &str = r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
signals = ["release", "issue"]
[reports.daily]
view = "queue"
[[routes]]
id = "everything"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "shown"
prereleases = "exclude"
[[routes]]
id = "board"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"
template = "board"
private = false
[templates.shown]
title = "{{ event.kind }} {{ event.project }} {{ event.body | length }}"
body = "shown"
[templates.board]
title = "{{ view.name }} of {{ event.attributes.pulls }}"
body = "{% for group in view.groups %}{{ group.name }}{% endfor %}"
"#;

fn kinds() -> Config {
    load(
        "herald.toml",
        KINDS,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

/// Each of `previewed` by its route, what it was rendered against and its
/// title.
fn shown(previewed: &[Previewed]) -> Vec<(&str, Option<Against>, String)> {
    previewed
        .iter()
        .map(|shown| {
            (
                shown.route.as_str(),
                shown.against.clone(),
                shown.rendered.as_ref().unwrap().title.plain().to_owned(),
            )
        })
        .collect()
}

fn sample(kind: &Kind, sample: Sample) -> Against {
    Against::Sample(kind.clone(), sample)
}

#[test]
fn each_route_renders_every_sample_of_each_kind_it_takes_and_each_report_a_sample_view() {
    let config = kinds();
    let issue = Kind::new("issue").unwrap();
    let plain = preview_samples(
        &config,
        Sampling {
            sample: Some(Sample::Plain),
            ..Sampling::default()
        },
    );
    assert_eq!(
        shown(&plain),
        [
            (
                "everything",
                Some(sample(&issue, Sample::Plain)),
                "issue sample-project 27".to_owned()
            ),
            (
                "everything",
                Some(sample(&Kind::RELEASE, Sample::Plain)),
                "release sample-project 27".to_owned()
            ),
            (
                "board",
                Some(Against::Report("daily".to_owned())),
                "queue of 2".to_owned()
            ),
        ]
    );
    let only = |sampling: Sampling<'_>| against(&config, sampling);
    let everything = |samples: &[(&Kind, Sample)]| -> Vec<(String, Option<Against>)> {
        samples
            .iter()
            .map(|(kind, which)| ("everything".to_owned(), Some(sample(kind, *which))))
            .collect()
    };
    let issues = everything(&[
        (&issue, Sample::Plain),
        (&issue, Sample::Long),
        (&issue, Sample::Bodiless),
    ]);
    let mut all = issues.clone();
    all.extend(everything(&[
        (&Kind::RELEASE, Sample::Plain),
        (&Kind::RELEASE, Sample::Prerelease),
        (&Kind::RELEASE, Sample::Long),
        (&Kind::RELEASE, Sample::Bodiless),
    ]));
    all.push((
        "board".to_owned(),
        Some(Against::Report("daily".to_owned())),
    ));
    assert_eq!(only(Sampling::default()), all);
    assert_eq!(
        only(Sampling {
            kind: Some(&issue),
            ..Sampling::default()
        }),
        issues
    );
    assert_eq!(
        only(Sampling {
            sample: Some(Sample::Prerelease),
            ..Sampling::default()
        }),
        everything(&[(&Kind::RELEASE, Sample::Prerelease)])
    );
}

/// `config`'s previews as `sampling` asks, by route and what each was
/// rendered against.
fn against(config: &Config, sampling: Sampling<'_>) -> Vec<(String, Option<Against>)> {
    preview_samples(config, sampling)
        .into_iter()
        .map(|shown| (shown.route.as_str().to_owned(), shown.against))
        .collect()
}

#[test]
fn a_report_is_rendered_showing_a_sample_view_alone_where_asked_for_reports_and_the_plain_sample() {
    let config = kinds();
    let issue = Kind::new("issue").unwrap();
    let only = |sampling: Sampling<'_>| against(&config, sampling);
    let board = RouteId::new("board");
    let reported = [(
        "board".to_owned(),
        Some(Against::Report("daily".to_owned())),
    )];
    for sampling in [
        Sampling {
            report: Some("daily"),
            ..Sampling::default()
        },
        Sampling {
            kind: Some(&Kind::REPORT),
            ..Sampling::default()
        },
        Sampling {
            only: Some(&board),
            sample: Some(Sample::Plain),
            ..Sampling::default()
        },
    ] {
        assert_eq!(only(sampling), reported);
    }
    let anything = RouteId::new("everything");
    for sampling in [
        Sampling {
            kind: Some(&Kind::REPORT),
            sample: Some(Sample::Long),
            ..Sampling::default()
        },
        Sampling {
            only: Some(&anything),
            report: Some("daily"),
            ..Sampling::default()
        },
        Sampling {
            report: Some("weekly"),
            ..Sampling::default()
        },
        Sampling {
            only: Some(&board),
            kind: Some(&issue),
            ..Sampling::default()
        },
    ] {
        assert_eq!(only(sampling), []);
    }
}

#[test]
fn a_grouping_route_renders_the_samples_of_each_kind_it_takes_as_one_group() {
    use crate::grouping::grouped;
    let config = grouped("", "");
    let group = |sampling: Sampling<'_>| {
        shown(&preview_samples(
            &config,
            Sampling {
                group: Some("v9"),
                ..sampling
            },
        ))
        .into_iter()
        .map(|(route, against, title)| (route.to_owned(), against, title))
        .collect::<Vec<_>>()
    };
    assert_eq!(
        group(Sampling::default()),
        [(
            "train".to_owned(),
            Some(Against::Group(4)),
            "v9 of 4".to_owned()
        )]
    );
    assert_eq!(
        group(Sampling {
            sample: Some(Sample::Long),
            ..Sampling::default()
        }),
        [(
            "train".to_owned(),
            Some(Against::Group(1)),
            "v9 of 1".to_owned()
        )]
    );
    let issue = Kind::new("issue").unwrap();
    assert_eq!(
        group(Sampling {
            kind: Some(&issue),
            ..Sampling::default()
        }),
        []
    );
    assert_eq!(
        preview_samples(
            &kinds(),
            Sampling {
                group: Some("v9"),
                ..Sampling::default()
            }
        ),
        []
    );
}

#[test]
fn an_event_is_rendered_only_by_the_routes_that_take_it() {
    use crate::grouping::{grouped, release};
    let config = kinds();
    let released = crate::harness::event("v1");
    assert_eq!(
        preview(&config, &released, None)
            .iter()
            .map(|shown| shown.route.as_str())
            .collect::<Vec<_>>(),
        ["everything"]
    );
    let candidate = herald_core::Event::normalise(herald_core::Draft {
        identity: "github.com/1/v2.0.0-rc.1".to_owned(),
        kind: Kind::RELEASE,
        project: "core".to_owned(),
        attributes: herald_core::versioned("v2.0.0-rc.1", true),
        title: "core".to_owned(),
        body: String::new(),
        links: std::collections::BTreeMap::new(),
        occurred: released.occurred(),
    })
    .unwrap();
    assert!(candidate.prerelease());
    assert_eq!(preview(&config, &candidate, None), []);
    let grouping = grouped("", "prereleases = \"exclude\"\n");
    let events = [release("cli", 2), candidate];
    let previewed = preview_group(&grouping, ("v9", &events), None);
    assert_eq!(
        previewed[0].rendered.as_ref().unwrap().title.plain(),
        "v9 of 1"
    );
    assert_eq!(preview_group(&grouping, ("v9", &events[1..]), None), []);
}
