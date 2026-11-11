//! What a Mastodon server says of an account when doctor asks, against its
//! fixtures (ADR-0004).

#[cfg(test)]
mod probe {
    use herald_core::Delivery;
    use herald_dest_fediverse::{Mastodon, RouteOptions};
    use herald_kinds::{Probe, Reach};
    use herald_ports::Secret;
    use herald_testing::{Fixture, Replay};
    use serde_json::json;

    fn mastodon(route: RouteOptions) -> Mastodon {
        Mastodon::new(
            "https://mastodon.example",
            Secret::new("HERALD_MASTODON_TOKEN", "test-token"),
            route,
        )
    }

    fn answered(statuses: Option<serde_json::Value>, verified: u16) -> Replay {
        let mut fixture =
            Fixture::from_json(include_str!("../fixtures/probe.reference.json")).unwrap();
        if verified != 200 {
            fixture.exchanges.truncate(1);
        }
        if let Some(exchange) = fixture.exchanges.first_mut() {
            exchange.response.status = verified;
        }
        if let (Some(statuses), Some(instance)) = (statuses, fixture.exchanges.get_mut(1)) {
            instance.response.body = Some(statuses);
        }
        Replay::of(fixture)
    }

    fn probed(route: RouteOptions, http: &Replay) -> Vec<Probe> {
        let probes = pollster::block_on(mastodon(route).probe(http, &Delivery::default()));
        http.finish().unwrap();
        probes
    }

    fn limited(most: usize, reserved: usize) -> serde_json::Value {
        json!({ "configuration": { "statuses": { "max_characters": most, "characters_reserved_per_url": reserved } } })
    }

    #[test]
    fn a_route_writing_less_than_its_server_takes_is_told_it_could_write_more() {
        let http = answered(None, 200);
        assert_eq!(
            probed(RouteOptions::default(), &http),
            [
                Probe::Taken,
                Probe::Said("the server takes 1000 characters in a status, more than the 500 herald writes; set limit = 1000 to use them".to_owned()),
            ]
        );
    }

    #[test]
    fn a_route_writing_more_than_its_server_takes_is_withheld() {
        let route = RouteOptions {
            limit: Some(800),
            link_length: Some(20),
            ..RouteOptions::default()
        };
        assert_eq!(
            probed(route.clone(), &answered(Some(limited(500, 23)), 200)),
            [
                Probe::Taken,
                Probe::Route {
                    key: Some("limit"),
                    why: "the server takes at most 500 characters in a status, fewer than the 800 herald writes; set limit = 500".to_owned(),
                },
                Probe::Route {
                    key: Some("link_length"),
                    why: "the server counts a link as 23 characters, more than the 20 herald counts; set link_length = 23".to_owned(),
                },
            ]
        );
        assert_eq!(
            probed(route, &answered(Some(limited(800, 20)), 200)),
            [Probe::Taken]
        );
        assert_eq!(
            probed(RouteOptions::default(), &answered(Some(limited(500, 10)), 200)),
            [
                Probe::Taken,
                Probe::Said("the server counts a link as 10 characters, fewer than the 23 herald counts; set link_length = 10".to_owned()),
            ]
        );
    }

    #[test]
    fn a_server_that_does_not_say_its_limits_leaves_them_unchecked() {
        let unsaid = "the server does not say how many characters a status holds at /api/v2/instance, so limit and link_length are not checked against it";
        assert_eq!(
            probed(RouteOptions::default(), &answered(Some(json!({})), 200)),
            [Probe::Taken, Probe::Said(unsaid.to_owned())]
        );
        let mut fixture =
            Fixture::from_json(include_str!("../fixtures/probe.reference.json")).unwrap();
        if let Some(instance) = fixture.exchanges.get_mut(1) {
            instance.response.status = 404;
        }
        assert_eq!(
            probed(RouteOptions::default(), &Replay::of(fixture)),
            [Probe::Taken, Probe::Said(unsaid.to_owned())]
        );
    }

    #[test]
    fn a_refused_token_is_all_that_is_asked() {
        assert_eq!(
            probed(RouteOptions::default(), &answered(None, 401)),
            [Probe::Credential(
                "Mastodon refuses its credential, answering 401".to_owned()
            )]
        );
    }
}
