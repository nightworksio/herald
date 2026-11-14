//! The apps' HTTP API: each event posted, scoped by its token, admitted once and announced, and where it stands.

use super::*;

fn posted<'a>(project: &'a str, headers: &'a [(&'a str, &'a str)], body: &'a [u8]) -> Inbound<'a> {
    Inbound {
        method: "POST",
        path: project,
        query: None,
        headers,
        body,
    }
}

/// A post's path, its headers, its body and the status it is answered with.
type Posted<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a [u8], u16);

const BEARER: &[(&str, &str)] = &[("Authorization", "Bearer app-token")];
const DEPLOYED: &[u8] = br#"{"id": "7", "project": "herald", "title": "Deployed"}"#;

#[test]
fn an_app_s_event_is_answered_and_left_to_announce_once() {
    let world = World::new(Script::default());
    let first = world.answer(&posted("/api/projects/herald/events", BEARER, DEPLOYED));
    assert_eq!(
        said(&first),
        (202, TEXT, "herald announces the event".to_owned())
    );
    let Some(Job::App {
        app,
        event,
        verified,
    }) = first.job
    else {
        panic!("the event is left to announce");
    };
    assert_eq!(app, "ci");
    assert_eq!(event.identity().as_str(), "app/ci/7");
    assert_eq!(event.project(), "herald");
    assert_eq!(verified.delivery.as_deref(), Some("app/ci/7"));
    assert_eq!(verified.signer.as_deref(), Some("ci"));
    let again = world.answer(&posted("/api/projects/herald/events", BEARER, DEPLOYED));
    assert_eq!(
        said(&again),
        (200, TEXT, "herald has this event already".to_owned())
    );
    assert_eq!(again.job, None);
    assert_eq!(
        world.admits(herald_sources_app::ADMITTED, "app/ci/7"),
        Admitted::Seen
    );
}

#[test]
fn an_app_s_event_its_token_or_scope_does_not_let_through_is_refused_unread() {
    let world = World::new(Script::default());
    let cases: [Posted<'_>; 9] = [
        ("/api/projects/herald/events", &[], DEPLOYED, 401),
        (
            "/api/projects/herald/events",
            &[("Authorization", "Bearer ")],
            DEPLOYED,
            401,
        ),
        (
            "/api/projects/herald/events",
            &[("Authorization", "app-token")],
            DEPLOYED,
            401,
        ),
        (
            "/api/projects/herald/events",
            &[("Authorization", "Bearer other")],
            DEPLOYED,
            401,
        ),
        ("/api/projects/other/events", BEARER, b"not json", 403),
        ("/api/projects/herald/events", BEARER, b"not json", 400),
        (
            "/api/projects/herald/events",
            BEARER,
            br#"{"id": "7", "project": "other", "title": "Deployed"}"#,
            400,
        ),
        (
            "/api/projects/herald/events",
            BEARER,
            br#"{"id": " ", "project": "herald", "title": "Deployed"}"#,
            400,
        ),
        ("/api/projects/herald", BEARER, DEPLOYED, 404),
    ];
    for (path, headers, body, status) in cases {
        let answered = world.answer(&posted(path, headers, body));
        assert_eq!(answered.reply.status, status, "{path} {headers:?}");
        assert_eq!(answered.job, None, "{path}");
        if status != 404 {
            assert_eq!(
                String::from_utf8(answered.reply.body).unwrap(),
                "herald does not take this request",
                "{path} {headers:?}"
            );
        }
    }
    let mismatched = world.answer(&posted(
        "/api/projects/herald/events",
        BEARER,
        br#"{"id": "7", "project": "other", "title": "Deployed"}"#,
    ));
    assert_eq!(
        mismatched.refused.as_deref(),
        Some("the event names the project \"other\", not \"herald\"")
    );
    assert_eq!(
        world.admits(herald_sources_app::ADMITTED, "app/ci/7"),
        Admitted::Fresh
    );
    let asked = Inbound {
        method: "GET",
        ..get("/api/projects/herald/events")
    };
    assert_eq!(world.answer(&asked).reply.status, 405);
}

#[test]
fn an_app_s_event_is_announced_to_the_routes_the_app_may_announce_to_only() {
    let world = World::new(Script::default());
    let event = herald_sources_app::event("ci", DEPLOYED, at(), &herald_core::KindSpecs::default())
        .unwrap();
    let Did::Announced(announced) = world.work(Job::App {
        app: "ci".to_owned(),
        event,
        verified: herald_flows::Verified {
            delivery: Some("app/ci/7".to_owned()),
            signer: Some("ci".to_owned()),
        },
    }) else {
        panic!("the event is announced");
    };
    let routes: Vec<&str> = announced
        .iter()
        .map(|announced| announced.route.as_str())
        .collect();
    assert_eq!(routes, ["deployed"]);
}

#[test]
fn an_app_s_work_that_stops_or_will_not_be_done_releases_its_delivery() {
    let world = World::failing(Fault::Claims);
    let job = || Job::App {
        app: "ci".to_owned(),
        event: herald_sources_app::event("ci", DEPLOYED, at(), &herald_core::KindSpecs::default())
            .unwrap(),
        verified: herald_flows::Verified {
            delivery: Some("app/ci/7".to_owned()),
            signer: Some("ci".to_owned()),
        },
    };
    assert_eq!(
        world.admits(herald_sources_app::ADMITTED, "app/ci/7"),
        Admitted::Fresh
    );
    let Did::Announced(announced) = world.work(job()) else {
        panic!("the event is announced");
    };
    assert_eq!(announced[0].outcome, unadmitted());
    assert_eq!(
        world.admits(herald_sources_app::ADMITTED, "app/ci/7"),
        Admitted::Fresh
    );
    assert_eq!(
        world.serving(|serving| pollster::block_on(unworked(serving, &job()))),
        Ok(())
    );
    assert_eq!(
        world.admits(herald_sources_app::ADMITTED, "app/ci/7"),
        Admitted::Fresh
    );
}

#[test]
fn where_an_app_s_event_stands_is_read_route_by_route_from_the_ledger() {
    let world = World::new(Script::default());
    let asked = Inbound {
        method: "GET",
        headers: BEARER,
        ..get("/api/projects/herald/events/7")
    };
    let (status, kind, body) = said(&world.answer(&asked));
    assert_eq!((status, kind), (200, "application/json"));
    assert_eq!(
        body,
        r#"{"id":"7","project":"herald","routes":[{"route":"deployed","standing":"pending"}]}"#
    );
    let event = herald_sources_app::event("ci", DEPLOYED, at(), &herald_core::KindSpecs::default())
        .unwrap();
    let identity = event.identity().clone();
    world.work(Job::App {
        app: "ci".to_owned(),
        event,
        verified: herald_flows::Verified {
            delivery: Some("app/ci/7".to_owned()),
            signer: Some("ci".to_owned()),
        },
    });
    let entry = pollster::block_on(
        world
            .store
            .inner
            .entry(&Key::derive(&RouteId::new("deployed"), &identity)),
    )
    .unwrap()
    .unwrap();
    assert_eq!(entry.value.wanted, Some(vec![Step::Post, Step::Deliveries]));
    assert_eq!(
        said(&world.answer(&asked)).2,
        r#"{"id":"7","project":"herald","routes":[{"route":"deployed","standing":"announced"}]}"#
    );
    for (headers, path, status) in [
        (&[][..], "/api/projects/herald/events/7", 401),
        (BEARER, "/api/projects/other/events/7", 403),
    ] {
        let refused = Inbound {
            method: "GET",
            headers,
            ..get(path)
        };
        assert_eq!(world.answer(&refused).reply.status, status, "{path}");
    }
    let put = Inbound {
        method: "PUT",
        ..asked
    };
    assert_eq!(world.answer(&put).reply.status, 405);
}

#[test]
fn where_an_event_stands_is_not_said_while_the_store_cannot_be_read() {
    let world = World::failing(Fault::Reads);
    let asked = Inbound {
        method: "GET",
        headers: BEARER,
        ..get("/api/projects/herald/events/7")
    };
    assert_eq!(world.answer(&asked).reply.status, 503);
}

#[test]
fn an_app_s_event_the_store_cannot_admit_is_answered_so_and_left_undone() {
    let world = World::failing(Fault::Admits);
    let answered = world.answer(&posted("/api/projects/herald/events", BEARER, DEPLOYED));
    assert_eq!(answered.reply.status, 503);
    assert_eq!(answered.job, None);
}

#[test]
fn a_project_and_an_id_are_read_from_their_percent_encoded_path_segments() {
    let world = World::new(Script::default());
    let event = br#"{"id": "deploy/7", "project": "night works", "title": "Deployed"}"#;
    let first = world.answer(&posted("/api/projects/night%20works/events", BEARER, event));
    world.work(first.job.expect("the event is left to announce"));
    let stood = r#"{"id":"deploy/7","project":"night works","routes":[{"route":"deployed","standing":"announced"}]}"#;
    for (path, status, body) in [
        ("/api/projects/night%20works/events/deploy%2F7", 200, stood),
        (
            "/api/projects/night%2works/events/7",
            404,
            "herald serves no such path here",
        ),
        (
            "/api/projects/night%20works/events/deploy/7",
            404,
            "herald serves no such path here",
        ),
    ] {
        let asked = Inbound {
            method: "GET",
            headers: BEARER,
            ..get(path)
        };
        let (code, _, text) = said(&world.answer(&asked));
        assert_eq!((code, text.as_str()), (status, body), "{path}");
    }
}
