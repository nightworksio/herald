use std::sync::Arc;

use herald_core::{
    Attribute, Cursor, Event, Identity, PACKAGE, PACKAGE_TYPE, Polled, Problem, RELEASE_LINK,
    Visibility,
};
use herald_kinds::{Poll, Probed, Unpolled};
use herald_testing::ScriptedHttp;
use jiff::Timestamp;

use super::super::image::{Image, invalid};
use super::super::session::{Failure, Reached};
use super::*;

const REGISTRY: &str = "https://registry.example";

const CONFIG: &str = "sha256:c0ffee";

const PLATFORM: &str = "sha256:beef";

fn image(options: &OciOptions, token: Option<Secret>) -> Image {
    let repository = options.repository.clone().unwrap_or_default();
    Image::new(
        Arc::new(Reached::new(options, token)),
        options.repository_of(&repository),
        options.visibility(),
    )
}

fn app() -> OciOptions {
    of(REGISTRY, "acme/app")
}

fn tags(tags: &[&str]) -> Response {
    answer(
        200,
        &serde_json::json!({ "name": "acme/app", "tags": tags }).to_string(),
    )
}

fn annotated(created: &str) -> Response {
    answer(
        200,
        &format!(
            r#"{{"annotations":{{"org.opencontainers.image.created":"{created}"}},"config":{{"digest":"{CONFIG}"}}}}"#
        ),
    )
}

fn index() -> Response {
    answer(
        200,
        &format!(r#"{{"manifests":[{{"digest":"{PLATFORM}"}}]}}"#),
    )
}

fn configured() -> Response {
    answer(200, &format!(r#"{{"config":{{"digest":"{CONFIG}"}}}}"#))
}

fn config(created: &str) -> Response {
    answer(200, &format!(r#"{{"created":"{created}"}}"#))
}

fn undated(response: Response) -> Response {
    Response {
        headers: Vec::new(),
        ..response
    }
}

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn date() -> Timestamp {
    at("2026-11-17T12:00:00Z")
}

fn poll(image: &Image, http: &ScriptedHttp, cursor: Option<&Cursor>) -> Result<Polled, Unpolled> {
    poll_at(image, http, cursor, date())
}

fn poll_at(
    image: &Image,
    http: &ScriptedHttp,
    cursor: Option<&Cursor>,
    now: Timestamp,
) -> Result<Polled, Unpolled> {
    pollster::block_on(image.polled(http, cursor, now)).map(Option::unwrap)
}

fn identity(tag: &str) -> Identity {
    Identity::new(format!("registry.example/acme/app/{tag}"))
}

fn identities(tags: &[&str]) -> Vec<Identity> {
    tags.iter().map(|tag| identity(tag)).collect()
}

fn tagged(events: &[Event]) -> Vec<(&str, Timestamp)> {
    events
        .iter()
        .map(|event| (event.version().unwrap_or_default(), event.occurred()))
        .collect()
}

fn cursor(seen: &[&str], newest: Option<Timestamp>) -> Cursor {
    Cursor {
        seen: identities(seen),
        newest,
        ..Cursor::default()
    }
}

fn urls(http: &ScriptedHttp) -> Vec<String> {
    http.asked().into_iter().map(|(url, _)| url).collect()
}

#[test]
fn a_first_poll_records_every_listed_tag_unread_page_by_page_leaving_moving_tags_out() {
    let options = OciOptions {
        listed_page: std::num::NonZeroUsize::new(2),
        ..app()
    };
    let image = image(&options, None);
    let http = ScriptedHttp::new(vec![
        tags(&["a", "b"]),
        with(
            tags(&["latest"]),
            "Link",
            r#"</v2/acme/app/tags/list?last=latest&n=2>; rel="next""#,
        ),
        tags(&["c"]),
    ]);
    let polled = poll(&image, &http, None).unwrap();
    assert_eq!(
        polled,
        Polled {
            unread: identities(&["a", "b", "c"]),
            ..Polled::default()
        }
    );
    assert_eq!(
        urls(&http),
        [
            "https://registry.example/v2/acme/app/tags/list?n=2",
            "https://registry.example/v2/acme/app/tags/list?n=2&last=b",
            "https://registry.example/v2/acme/app/tags/list?n=2&last=latest",
        ]
    );
    assert_eq!(
        header(&http.requests()[0], "Accept").as_deref(),
        Some("application/json")
    );
}

#[test]
fn a_first_poll_with_a_backfill_reads_the_last_page_of_tags_newest_first() {
    let options = OciOptions {
        backfill: 1,
        page: std::num::NonZeroUsize::new(2),
        ..app()
    };
    let image = image(&options, None);
    let http = ScriptedHttp::new(vec![
        tags(&["a", "b", "c"]),
        annotated("2025-03-01T00:00:00Z"),
        index(),
        configured(),
        config("1970-01-01T00:00:00Z"),
    ]);
    let polled = poll(&image, &http, None).unwrap();
    assert_eq!(
        tagged(&polled.events),
        [("b", date()), ("c", at("2025-03-01T00:00:00Z"))]
    );
    assert_eq!(polled.unread, identities(&["a"]));
    assert_eq!(polled.newest, Some(date()));
    assert_eq!(
        urls(&http)[1..],
        [
            "https://registry.example/v2/acme/app/manifests/c",
            "https://registry.example/v2/acme/app/manifests/b",
            "https://registry.example/v2/acme/app/manifests/sha256:beef",
            "https://registry.example/v2/acme/app/blobs/sha256:c0ffee",
        ]
    );
    assert_eq!(
        header(&http.requests()[1], "Accept").as_deref(),
        Some(super::super::wire::MANIFESTS)
    );
    let event = &polled.events[0];
    assert_eq!(event.identity(), &identity("b"));
    assert_eq!(event.project(), "acme/app");
    assert_eq!(event.title(), "acme/app b");
    assert_eq!(event.kind(), &herald_core::Kind::PACKAGE_VERSION);
    assert_eq!(
        event.attributes().get(PACKAGE),
        Some(&Attribute::Text("app".to_owned()))
    );
    assert_eq!(
        event.attributes().get(PACKAGE_TYPE),
        Some(&Attribute::Text("container".to_owned()))
    );
    assert!(event.links().is_empty());
    assert_eq!(event.visibility(), Some(Visibility::Public));
    let origin = event.origin().unwrap();
    assert_eq!(origin.forge, "oci");
    assert_eq!(origin.host, "registry.example");
    assert_eq!(origin.owner, "acme");
    assert_eq!(origin.repository, "acme/app");
    assert_eq!(origin.visibility, Visibility::Public);
}

#[test]
fn a_tag_older_than_the_newest_announced_is_recorded_unannounced_and_the_mark_advances() {
    let image = image(&app(), None);
    let http = ScriptedHttp::new(vec![
        tags(&["a", "b", "c", "d", "e"]),
        annotated("2025-01-01T00:00:00Z"),
        annotated("2026-01-01T00:00:00Z"),
    ]);
    let seen = cursor(&["c", "b", "a"], Some(at("2025-06-01T00:00:00Z")));
    let polled = poll(&image, &http, Some(&seen)).unwrap();
    assert_eq!(tagged(&polled.events), [("d", at("2026-01-01T00:00:00Z"))]);
    assert_eq!(polled.unread, identities(&["a", "b", "c", "e"]));
    assert_eq!(polled.newest, Some(at("2026-01-01T00:00:00Z")));
    let same = ScriptedHttp::new(vec![tags(&["a", "b"]), annotated("2025-06-01T00:00:00Z")]);
    let marked = cursor(&["a"], Some(at("2025-06-01T00:00:00Z")));
    let polled = poll(&image, &same, Some(&marked)).unwrap();
    assert_eq!(tagged(&polled.events), [("b", at("2025-06-01T00:00:00Z"))]);
    assert_eq!(polled.newest, Some(at("2025-06-01T00:00:00Z")));
}

#[test]
fn unseen_tags_past_a_page_wait_for_the_next_poll_and_keep_the_mark() {
    let options = OciOptions {
        page: std::num::NonZeroUsize::new(1),
        ..app()
    };
    let image = image(&options, None);
    let http = ScriptedHttp::new(vec![
        tags(&["a", "b", "c"]),
        annotated("2026-01-01T00:00:00Z"),
    ]);
    let polled = poll(&image, &http, Some(&cursor(&["a"], None))).unwrap();
    assert_eq!(tagged(&polled.events), [("c", at("2026-01-01T00:00:00Z"))]);
    assert_eq!(polled.unread, identities(&["a"]));
    assert_eq!(polled.newest, None);
    let last = ScriptedHttp::new(vec![
        tags(&["a", "b", "c"]),
        annotated("2026-02-01T00:00:00Z"),
    ]);
    let polled = poll(&image, &last, Some(&cursor(&["a", "c"], None))).unwrap();
    assert_eq!(polled.newest, Some(at("2026-02-01T00:00:00Z")));
}

#[test]
fn a_cursor_that_saw_nothing_reads_a_page_and_records_the_rest() {
    let options = OciOptions {
        page: std::num::NonZeroUsize::new(1),
        ..app()
    };
    let image = image(&options, None);
    let http = ScriptedHttp::new(vec![tags(&["a", "b"]), annotated("2026-01-01T00:00:00Z")]);
    let polled = poll(&image, &http, Some(&Cursor::default())).unwrap();
    assert_eq!(tagged(&polled.events), [("b", at("2026-01-01T00:00:00Z"))]);
    assert_eq!(polled.unread, identities(&["a"]));
    assert_eq!(polled.newest, Some(at("2026-01-01T00:00:00Z")));
}

#[test]
fn a_tag_gone_before_it_is_read_is_left_out_and_one_no_answer_dates_takes_the_round_s_time() {
    let image = image(&app(), None);
    let http = ScriptedHttp::new(vec![
        undated(tags(&["a", "b", "c"])),
        answer(404, ""),
        undated(index()),
        undated(answer(
            200,
            &format!(r#"{{"manifests":[{{"digest":"{PLATFORM}"}}]}}"#),
        )),
    ]);
    let later = at("2026-11-17T13:00:00Z");
    let polled = poll_at(&image, &http, Some(&cursor(&["a"], None)), later).unwrap();
    assert_eq!(tagged(&polled.events), [("b", later)]);
    assert_eq!(polled.unread, identities(&["a"]));
    assert_eq!(polled.newest, Some(later));
}

#[test]
fn a_tag_whose_blob_is_redirected_elsewhere_takes_the_time_it_was_first_seen() {
    let image = image(&app(), None);
    let http = ScriptedHttp::new(vec![
        tags(&["a"]),
        configured(),
        with(answer(307, ""), "Location", "https://cdn.example/blob"),
    ]);
    let polled = poll(&image, &http, Some(&cursor(&["z"], None))).unwrap();
    assert_eq!(tagged(&polled.events), [("a", date())]);
    let listed_only = ScriptedHttp::new(vec![
        tags(&["a"]),
        undated(configured()),
        undated(config("1970-01-01T00:00:00Z")),
    ]);
    let polled = poll(&image, &listed_only, Some(&cursor(&["z"], None))).unwrap();
    assert_eq!(tagged(&polled.events), [("a", date())]);
}

#[test]
fn a_listing_over_most_tags_or_repeating_a_tag_refuses_the_poll() {
    let capped = OciOptions {
        most_tags: std::num::NonZeroUsize::new(2),
        ..app()
    };
    let full = ScriptedHttp::new(vec![tags(&["a", "b"])]);
    assert_eq!(
        poll(&image(&capped, None), &full, None).unwrap().unread,
        identities(&["a", "b"])
    );
    let http = ScriptedHttp::new(vec![tags(&["a", "b", "c"])]);
    assert_eq!(
        poll(&image(&capped, None), &http, None).unwrap_err(),
        Unpolled::Unreadable(
            "acme/app lists more than 2 tags, most_tags; raise it, or name another repository"
                .to_owned()
        )
    );
    let paged = OciOptions {
        listed_page: std::num::NonZeroUsize::new(2),
        ..app()
    };
    let http = ScriptedHttp::new(vec![tags(&["a", "b"]), tags(&["b", "c"])]);
    assert_eq!(
        poll(&image(&paged, None), &http, None).unwrap_err(),
        Unpolled::Unreadable(
            "the registry listed acme/app's tag \"b\" twice; it does not page its tags by last"
                .to_owned()
        )
    );
}

#[test]
fn a_listing_manifest_or_config_the_registry_does_not_answer_refuses_the_poll() {
    let image = image(&app(), None);
    for answers in [
        vec![answer(500, "")],
        vec![answer(200, "[]")],
        vec![tags(&["a"]), answer(500, "")],
        vec![tags(&["a"]), index(), answer(200, "{}")],
        vec![tags(&["a"]), configured(), answer(500, "")],
        vec![tags(&["a"]), configured(), answer(200, "[]")],
    ] {
        let http = ScriptedHttp::new(answers);
        assert!(matches!(
            poll(&image, &http, Some(&cursor(&["z"], None))),
            Err(Unpolled::Unreadable(_))
        ));
    }
    let silent = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        poll(&image, &silent, None),
        Err(Unpolled::Unreachable(_))
    ));
}

#[test]
fn docker_hub_s_official_image_announces_as_its_name_on_docker_io_linking_its_page() {
    let options = of("https://registry-1.docker.io", "caddy");
    let image = image(&options, None);
    let http = ScriptedHttp::new(vec![
        with(
            answer(401, ""),
            "WWW-Authenticate",
            r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/caddy:pull""#,
        ),
        answer(200, r#"{"token":"anon"}"#),
        tags(&["2.10.0"]),
        annotated("2025-07-15T19:29:15Z"),
    ]);
    let polled = poll(&image, &http, Some(&Cursor::default())).unwrap();
    let event = &polled.events[0];
    assert_eq!(event.identity().as_str(), "docker.io/library/caddy/2.10.0");
    assert_eq!(event.project(), "caddy");
    assert_eq!(event.title(), "caddy 2.10.0");
    assert_eq!(
        event.links().get(RELEASE_LINK).map(String::as_str),
        Some("https://hub.docker.com/_/caddy/tags?name=2.10.0")
    );
    assert_eq!(
        event.attributes().get(PACKAGE),
        Some(&Attribute::Text("caddy".to_owned()))
    );
    let origin = event.origin().unwrap();
    assert_eq!(
        (origin.host.as_str(), origin.owner.as_str()),
        ("docker.io", "library")
    );
    assert_eq!(
        urls(&http)[2],
        "https://registry-1.docker.io/v2/library/caddy/tags/list?n=1000"
    );
}

#[test]
fn a_private_image_links_no_page_and_is_seen_as_private() {
    let options = OciOptions {
        username: Some("me".to_owned()),
        token: Some(token()),
        ..of("https://registry-1.docker.io", "acme/app")
    };
    let image = image(&options, Some(read_token()));
    let http = ScriptedHttp::new(vec![tags(&["1.0"]), annotated("2025-07-15T19:29:15Z")]);
    let polled = poll(&image, &http, Some(&Cursor::default())).unwrap();
    let event = &polled.events[0];
    assert!(event.links().is_empty());
    assert_eq!(event.visibility(), Some(Visibility::Private));
    assert_eq!(event.origin().unwrap().visibility, Visibility::Private);
    assert_eq!(event.project(), "acme/app");
    let single = image_named("app");
    assert_eq!(single.origin().owner, "");
    assert_eq!(single.project(), "app");
}

fn image_named(repository: &str) -> Image {
    image(&of(REGISTRY, repository), None)
}

#[test]
fn an_index_naming_another_index_takes_the_time_its_tag_was_first_seen() {
    let image = image(&app(), None);
    let http = ScriptedHttp::new(vec![tags(&["a"]), index(), index()]);
    let polled = poll(&image, &http, Some(&cursor(&["z"], None))).unwrap();
    assert_eq!(tagged(&polled.events), [("a", date())]);
}

#[test]
fn a_tag_making_no_event_says_why() {
    assert_eq!(
        invalid(Problem::NoTitle),
        Failure::at(
            "repository",
            "the registry's tags of a repository makes no event: the event has no title"
        )
    );
}

#[test]
fn an_image_reaches_its_registry_and_remembers_its_tags() {
    let image = image(&of("https://registry-1.docker.io", "caddy"), None);
    assert_eq!(image.api(), "https://registry-1.docker.io");
    assert_eq!(image.bases().len(), 4);
    assert_eq!(image.remembered(), 10_000);
    assert_eq!(image.tag_window(), None);
}

fn probed(image: &Image, answers: Vec<Response>) -> Vec<Probed> {
    let http = ScriptedHttp::new(answers);
    pollster::block_on(image.probed(&http))
}

#[test]
fn checking_an_image_reads_its_first_tag_and_names_a_blob_redirected_where_blobs_does_not_reach() {
    let image = image(&app(), None);
    assert_eq!(
        probed(
            &image,
            vec![tags(&["1.0"]), annotated("2025-01-01T00:00:00Z")]
        ),
        []
    );
    assert_eq!(probed(&image, vec![tags(&[])]), []);
    assert_eq!(
        probed(
            &image,
            vec![
                tags(&["1.0"]),
                configured(),
                with(answer(302, ""), "Location", "https://cdn.example/b"),
            ]
        ),
        [Probed {
            key: "blobs",
            why: "the registry redirects acme/app's config blobs to https://cdn.example, which blobs does not name, so its tags take the time they are first seen; add it to blobs where it is the registry's own".to_owned()
        }]
    );
    let refused = probed(
        &image,
        vec![with(
            answer(401, ""),
            "WWW-Authenticate",
            r#"Bearer realm="https://evil.example/token""#,
        )],
    );
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].key, "auth");
    for signed in [
        "https://cdn.example/b?X-Amz-Signature=s3cret",
        "//cdn.example/b?X-Amz-Signature=s3cret",
    ] {
        let named = probed(
            &image,
            vec![
                tags(&["1.0"]),
                configured(),
                with(answer(307, ""), "Location", signed),
            ],
        );
        assert_eq!(named.len(), 1);
        assert!(!format!("{named:?}").contains("s3cret"), "{named:?}");
        let http = ScriptedHttp::new(vec![
            tags(&["1.0"]),
            configured(),
            with(answer(307, ""), "Location", signed),
        ]);
        let polled = poll(&image, &http, Some(&cursor(&["z"], None))).unwrap();
        assert!(!format!("{polled:?}").contains("s3cret"), "{polled:?}");
    }
    let unlisted = probed(&image, vec![answer(500, "")]);
    assert_eq!(unlisted[0].key, "repository");
    let unread = probed(&image, vec![tags(&["1.0"]), answer(500, "")]);
    assert_eq!(unread[0].key, "repository");
    let http = ScriptedHttp::new(vec![tags(&["1.0"]), annotated("2025-01-01T00:00:00Z")]);
    pollster::block_on(image.probed(&http));
    assert_eq!(
        urls(&http)[0],
        "https://registry.example/v2/acme/app/tags/list?n=1"
    );
}

#[test]
fn a_time_past_the_round_s_is_not_believed_so_a_future_tag_suppresses_no_later_one() {
    let image = image(&app(), None);
    let future = ScriptedHttp::new(vec![tags(&["a", "b"]), annotated("2100-01-01T00:00:00Z")]);
    let polled = poll(&image, &future, Some(&cursor(&["a"], None))).unwrap();
    assert_eq!(tagged(&polled.events), [("b", date())]);
    assert_eq!(polled.newest, Some(date()));
    let ahead = ScriptedHttp::new(vec![tags(&["a", "b"]), annotated("2026-11-17T12:05:00Z")]);
    let polled = poll(&image, &ahead, Some(&cursor(&["a"], None))).unwrap();
    assert_eq!(tagged(&polled.events), [("b", at("2026-11-17T12:05:00Z"))]);
    let past = ScriptedHttp::new(vec![tags(&["a", "b"]), annotated("2026-11-17T12:05:01Z")]);
    let polled = poll(&image, &past, Some(&cursor(&["a"], None))).unwrap();
    assert_eq!(tagged(&polled.events), [("b", date())]);
    let next_day = at("2026-11-18T12:00:00Z");
    let real = ScriptedHttp::new(vec![
        tags(&["a", "b", "c"]),
        annotated("2026-11-18T00:00:00Z"),
    ]);
    let polled = poll_at(
        &image,
        &real,
        Some(&cursor(&["a", "b"], Some(date()))),
        next_day,
    )
    .unwrap();
    assert_eq!(tagged(&polled.events), [("c", at("2026-11-18T00:00:00Z"))]);
    let lying =
        |response: Response| with(undated(response), "Date", "Fri, 01 Jan 2100 00:00:00 GMT");
    for answers in [
        vec![
            undated(tags(&["a", "b"])),
            undated(annotated("2100-01-01T00:00:00Z")),
        ],
        vec![
            lying(tags(&["a", "b"])),
            lying(annotated("2100-01-01T00:00:00Z")),
        ],
    ] {
        let http = ScriptedHttp::new(answers);
        let polled = poll(&image, &http, Some(&cursor(&["a"], None))).unwrap();
        assert_eq!(tagged(&polled.events), [("b", date())]);
        assert_eq!(polled.newest, Some(date()));
    }
}

#[test]
fn an_undated_tag_takes_the_time_its_manifest_was_answered_not_its_listing() {
    let image = image(&app(), None);
    let later =
        |response: Response| with(undated(response), "Date", "Tue, 17 Nov 2026 12:03:00 GMT");
    let http = ScriptedHttp::new(vec![
        tags(&["a"]),
        later(configured()),
        config("1970-01-01T00:00:00Z"),
    ]);
    let polled = poll_at(
        &image,
        &http,
        Some(&cursor(&["z"], None)),
        at("2026-11-17T12:04:00Z"),
    )
    .unwrap();
    assert_eq!(tagged(&polled.events), [("a", at("2026-11-17T12:03:00Z"))]);
}
