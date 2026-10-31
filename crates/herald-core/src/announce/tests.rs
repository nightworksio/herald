use std::collections::BTreeMap;

use super::*;
use crate::event::Draft;
use crate::event::tests::{draft, event};
use crate::profile::{Dialect, Features, Limits, MentionStyle, Threads};
use crate::render::Form;
use crate::route::Thread;
use crate::route::tests::route_for;

fn profile(title: usize, text: usize) -> Profile {
    Profile {
        dialect: Dialect::Plain,
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
            pins: true,
            reads_back: true,
            edits: true,
            mentions: MentionStyle::Roles,
            interactions: true,
        },
    }
}

fn marker() -> Marker {
    Marker::of(b"mac")
}

fn templates(route: &Route) -> Templates {
    Templates::compile(&[
        (
            &title_template(&route.template),
            "📦 {{ event.project }} {{ event.version }}",
            Form::Line,
        ),
        (
            &body_template(&route.template),
            "{{ event.body }}",
            Form::Markdown,
        ),
        (
            &thread_template(&route.id),
            "{{ event.project }} {{ event.version }}",
            Form::Line,
        ),
    ])
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
        marker(),
    )
    .unwrap();
    assert_eq!(announcement.key, Key::derive(&route.id, event().identity()));
    assert_eq!(announcement.marker, marker());
    assert_eq!(announcement.route, route.id);
    assert_eq!(announcement.title, "📦 php-mutation-gate v0.1.0");
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
    assert!(!announcement.prerelease);
    assert_eq!(announcement.occurred, event().occurred());
}

#[test]
fn a_route_without_a_thread_names_none_and_a_long_body_is_cut() {
    let route = route_for("r", "php-mutation-gate");
    let long = Event::normalise(Draft {
        body: "one\n\ntwo\n\nthree".to_owned(),
        prerelease: true,
        ..draft()
    })
    .unwrap();
    let announcement = announce(
        &long,
        &route,
        &templates(&route),
        &profile(10, 5),
        "0.1.0",
        marker(),
    )
    .unwrap();
    assert_eq!(announcement.thread, None);
    assert_eq!(announcement.title, "📦 php-mut…");
    assert_eq!(announcement.parts.len(), 3);
    assert!(announcement.prerelease);
}

#[test]
fn without_a_release_link_the_first_link_is_used() {
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
        marker(),
    )
    .unwrap();
    assert_eq!(announcement.link.as_deref(), Some("https://example.org/c"));
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
            marker()
        )
        .unwrap()
        .link,
        None
    );
}

#[test]
fn a_template_that_fails_fails_the_announcement() {
    let route = threaded();
    let broken = Templates::compile(&[]).unwrap();
    assert!(
        announce(
            &event(),
            &route,
            &broken,
            &profile(256, 4096),
            "0",
            marker()
        )
        .is_err()
    );
    let no_body =
        Templates::compile(&[(&title_template(&route.template), "t", Form::Line)]).unwrap();
    assert!(
        announce(
            &event(),
            &route,
            &no_body,
            &profile(256, 4096),
            "0",
            marker()
        )
        .is_err()
    );
    let no_thread = Templates::compile(&[
        (&title_template(&route.template), "t", Form::Line),
        (&body_template(&route.template), "b", Form::Markdown),
    ])
    .unwrap();
    assert!(
        announce(
            &event(),
            &route,
            &no_thread,
            &profile(256, 4096),
            "0",
            marker()
        )
        .is_err()
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
