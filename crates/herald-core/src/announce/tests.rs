use std::collections::BTreeMap;

use super::*;
use crate::event::Draft;
use crate::event::tests::{draft, event};
use crate::marker::Marker;
use crate::profile::fixtures::{DISCORD, MATRIX, PLAIN, SLACK};
use crate::profile::{Features, Limits, MentionStyle, Pins, Threads};
use crate::render::Form;
use crate::route::Thread;
use crate::route::tests::route_for;

fn profile(title: usize, text: usize) -> Profile {
    Profile {
        markup: PLAIN,
        measure: Measure::Chars,
        limits: Limits {
            title,
            text,
            followup: text,
            thread_name: 12,
        },
        features: Features {
            threads: Threads::FromMessage,
            crosspost: true,
            pins: Pins::Many,
            reads_back: true,
            edits: true,
            mentions: MentionStyle::Roles,
            interactions: true,
        },
        symbols: crate::Symbols::DEFAULT,
    }
}

fn marks() -> Marks {
    Marks {
        route: Marker::of(b"route"),
        announcement: Marker::of(b"mac"),
    }
}

fn links() -> Vec<LinkSpec> {
    vec![LinkSpec::named("release"), LinkSpec::named("changelog")]
}

fn templates(route: &Route) -> Templates {
    Templates::compile(
        &[
            (
                &title_template(&route.template),
                "📦 {{ event.project }} {{ event.attributes.version }}",
                Form::Line,
            ),
            (
                &body_template(&route.template),
                "{{ event.body }}",
                Form::Markdown,
            ),
            (
                &thread_template(&route.id),
                "{{ event.project }} {{ event.attributes.version }}",
                Form::Line,
            ),
        ],
        crate::Sandbox::DEFAULT,
    )
    .unwrap()
}

fn threaded() -> Route {
    let mut route = route_for("php-mutation-gate-releases", "php-mutation-gate");
    route.delivery.thread = Some(Thread {
        name: String::new(),
        archive_after: None,
        lock_after: None,
    });
    route
}

#[test]
fn an_event_is_announced_with_its_key_title_parts_thread_and_link() {
    let route = threaded();
    let announcement = announce(
        &event(),
        &route,
        &templates(&route),
        &profile(256, 4096),
        "0.1.0",
        marks(),
        &links(),
    )
    .unwrap();
    assert_eq!(announcement.key, Key::derive(&route.id, event().identity()));
    assert_eq!(announcement.marks, marks());
    assert_eq!(announcement.replaces, None);
    assert_eq!(announcement.route, route.id);
    assert_eq!(announcement.title.plain(), "📦 php-mutation-gate v0.1.0");
    assert_eq!(
        announcement
            .parts
            .iter()
            .map(|part| part.body.as_str())
            .collect::<Vec<_>>(),
        ["Added\n\n- the gate"]
    );
    assert_eq!(announcement.thread.as_deref(), Some("php-mutatio…"));
    assert_eq!(
        announcement.link.as_deref(),
        Some("https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0")
    );
    assert_eq!(
        announcement.buttons,
        [Button {
            name: "release".to_owned(),
            label: None,
            url: announcement.link.clone().unwrap(),
        }]
    );
    assert!(!announcement.prerelease);
    assert_eq!(announcement.occurred, event().occurred());
}

#[test]
fn each_list_of_users_past_the_room_is_a_message_after_the_parts() {
    let route = threaded();
    let mut announcement = announce(
        &event(),
        &route,
        &templates(&route),
        &profile(256, 4096),
        "0.1.0",
        marks(),
        &links(),
    )
    .unwrap();
    announcement.mentioned_after = vec![vec!["a".to_owned()], vec!["b".to_owned(), "c".to_owned()]];
    assert_eq!(announcement.messages(), 3);
    assert_eq!(announcement.mentioned_in(0), None);
    assert_eq!(announcement.mentioned_in(1), Some(&["a".to_owned()][..]));
    assert_eq!(
        announcement.mentioned_in(2),
        Some(&["b".to_owned(), "c".to_owned()][..])
    );
    assert_eq!(announcement.mentioned_in(3), None);
}

#[test]
fn a_route_without_a_thread_names_none_and_a_long_body_is_cut() {
    let route = route_for("r", "php-mutation-gate");
    let long = Event::normalise(Draft {
        body: "one\n\ntwo\n\nthree".to_owned(),
        attributes: crate::versioned("v0.1.0", true),
        ..draft()
    })
    .unwrap();
    let announcement = announce(
        &long,
        &route,
        &templates(&route),
        &profile(10, 5),
        "0.1.0",
        marks(),
        &links(),
    )
    .unwrap();
    assert_eq!(announcement.thread, None);
    assert_eq!(announcement.title.plain(), "📦 php-mut…");
    assert_eq!(announcement.parts.len(), 3);
    assert!(announcement.prerelease);
}

#[test]
fn without_a_release_link_the_first_link_is_used_and_the_changelog_kept() {
    let route = route_for("r", "php-mutation-gate");
    let other = Event::normalise(Draft {
        links: BTreeMap::from([
            ("download".to_owned(), "https://example.org/d".to_owned()),
            ("changelog".to_owned(), "https://example.org/c".to_owned()),
        ]),
        ..draft()
    })
    .unwrap();
    let announcement = announce(
        &other,
        &route,
        &templates(&route),
        &profile(256, 4096),
        "0",
        marks(),
        &links(),
    )
    .unwrap();
    assert_eq!(announcement.link.as_deref(), Some("https://example.org/c"));
    assert_eq!(
        announcement.buttons,
        [Button {
            name: "changelog".to_owned(),
            label: None,
            url: "https://example.org/c".to_owned(),
        }]
    );
    let bare = Event::normalise(Draft {
        links: BTreeMap::new(),
        ..draft()
    })
    .unwrap();
    assert_eq!(
        announce(
            &bare,
            &route,
            &templates(&route),
            &profile(256, 4096),
            "0",
            marks(),
            &links(),
        )
        .unwrap()
        .link,
        None
    );
}

#[test]
fn an_announcement_is_written_with_its_destination_s_symbols() {
    let route = threaded();
    let profile = Profile {
        symbols: crate::Symbols {
            bullet: Some('*'),
            ..crate::Symbols::DEFAULT
        },
        ..profile(256, 4096)
    };
    let announcement = announce(
        &event(),
        &route,
        &templates(&route),
        &profile,
        "0.1.0",
        marks(),
        &links(),
    )
    .unwrap();
    assert_eq!(announcement.parts[0].body, "Added\n\n* the gate");
}

#[test]
fn a_template_that_fails_fails_the_announcement() {
    let route = threaded();
    let broken = Templates::compile(&[], crate::Sandbox::DEFAULT).unwrap();
    assert_eq!(
        announce(
            &event(),
            &route,
            &broken,
            &profile(256, 4096),
            "0",
            marks(),
            &links()
        )
        .unwrap_err()
        .to_string(),
        "template package-release/title: template not found: template \"package-release/title\" does not exist"
    );
    let no_body = Templates::compile(
        &[(&title_template(&route.template), "t", Form::Line)],
        crate::Sandbox::DEFAULT,
    )
    .unwrap();
    assert_eq!(
        announce(
            &event(),
            &route,
            &no_body,
            &profile(256, 4096),
            "0",
            marks(),
            &links(),
        )
        .unwrap_err()
        .to_string(),
        "template package-release/body: template not found: template \"package-release/body\" does not exist"
    );
    let no_thread = Templates::compile(
        &[
            (&title_template(&route.template), "t", Form::Line),
            (&body_template(&route.template), "b", Form::Markdown),
        ],
        crate::Sandbox::DEFAULT,
    )
    .unwrap();
    assert_eq!(
        announce(
            &event(),
            &route,
            &no_thread,
            &profile(256, 4096),
            "0",
            marks(),
            &links(),
        )
        .unwrap_err()
        .to_string(),
        "template route/php-mutation-gate-releases/thread: template not found: template \"route/php-mutation-gate-releases/thread\" does not exist"
    );
}

#[test]
fn text_is_shortened_between_graphemes_with_an_ellipsis() {
    assert_eq!(shortened("short", 5, Measure::Chars), "short");
    assert_eq!(shortened("a long title", 7, Measure::Chars), "a long…");
    assert_eq!(shortened("éééé", 5, Measure::Bytes), "é…");
    assert_eq!(
        shortened("👨\u{200d}👩\u{200d}👧 family", 4, Measure::Chars),
        "…"
    );
}

#[test]
fn template_names_are_namespaced_by_part() {
    assert_eq!(title_template(&TemplateName::new("t")), "t/title");
    assert_eq!(body_template(&TemplateName::new("t")), "t/body");
    assert_eq!(thread_template(&RouteId::new("r")), "route/r/thread");
}

#[test]
fn a_title_is_plain_as_written_and_shown_inert_in_every_dialect() {
    use crate::markup::inert::literal_inert;
    for written in [
        "v2 @alice@mastodon.social #tag",
        "<@123> <!everyone> **bold** [a](https://evil.example) @room",
        "a <b> & c",
    ] {
        let title = Title::new(written);
        assert_eq!(title.plain(), written);
        for markup in [DISCORD, SLACK, MATRIX, PLAIN] {
            let shown = title.shown(markup);
            assert_eq!(
                literal_inert(&shown, markup),
                Ok(()),
                "{written:?} in {markup:?}"
            );
        }
    }
}

#[test]
fn a_button_is_labelled_by_the_config_or_as_herald_words_its_link_or_by_its_name() {
    let button = |name: &str, label: Option<&str>| Button {
        name: name.to_owned(),
        label: label.map(ToOwned::to_owned),
        url: "https://example.org".to_owned(),
    };
    let wording = Wording::default();
    assert_eq!(button("release", None).label(&wording), "Release");
    assert_eq!(button("changelog", None).label(&wording), "Changelog");
    assert_eq!(button("entry", None).label(&wording), "Read");
    assert_eq!(button("status", None).label(&wording), "status");
    assert_eq!(button("release", Some("Notes")).label(&wording), "Notes");
    let dutch = Wording::of(Some(&BTreeMap::from([(
        Phrase::EntryLink,
        "Lezen".to_owned(),
    )])));
    assert_eq!(button("entry", None).label(&dutch), "Lezen");
}

#[test]
fn a_kind_s_links_become_buttons_in_its_order_where_the_event_carries_them() {
    let route = threaded();
    let linked = Event::normalise(Draft {
        links: BTreeMap::from([
            ("changelog".to_owned(), "https://example.org/c".to_owned()),
            ("status".to_owned(), "https://example.org/s".to_owned()),
            ("release".to_owned(), "https://example.org/r".to_owned()),
        ]),
        ..draft()
    })
    .unwrap();
    let order = [
        LinkSpec {
            name: "status".to_owned(),
            label: Some("Status page".to_owned()),
        },
        LinkSpec::named("missing"),
        LinkSpec::named("release"),
    ];
    let announcement = announce(
        &linked,
        &route,
        &templates(&route),
        &profile(256, 4096),
        "0",
        marks(),
        &order,
    )
    .unwrap();
    let buttons: Vec<(&str, Option<&str>, &str)> = announcement
        .buttons
        .iter()
        .map(|button| {
            (
                button.name.as_str(),
                button.label.as_deref(),
                button.url.as_str(),
            )
        })
        .collect();
    assert_eq!(
        buttons,
        [
            ("status", Some("Status page"), "https://example.org/s"),
            ("release", None, "https://example.org/r"),
        ]
    );
}
