//! What Slack says of a bot token when doctor asks, against its fixtures.

#[cfg(test)]
mod probe {
    use herald_core::{Delivery, Pinning};
    use herald_dest_slack::{Credential, RouteOptions, Slack};
    use herald_kinds::{Probe, Reach};
    use herald_ports::Secret;
    use herald_testing::{Fixture, Replay};
    use serde_json::{Value, json};

    fn slack(team: Option<&str>) -> Slack {
        Slack::new(
            Credential::Bot(Secret::new("HERALD_SLACK_TOKEN", "xoxb-test")),
            RouteOptions {
                channel: Some("C0123456789".to_owned()),
                subscribe: None,
            },
            None,
        )
        .in_team(team)
    }

    fn answered(status: u16, body: Option<Value>) -> Replay {
        let mut fixture =
            Fixture::from_json(include_str!("../fixtures/probe.reference.json")).unwrap();
        if let Some(exchange) = fixture.exchanges.first_mut() {
            exchange.response.status = status;
            if let Some(body) = body {
                exchange.response.body = Some(body);
            }
        }
        Replay::of(fixture)
    }

    fn probed(slack: &Slack, http: &Replay, pinning: bool) -> Vec<Probe> {
        let delivery = Delivery {
            pin: pinning.then_some(Pinning::Store),
            ..Delivery::default()
        };
        let probes = pollster::block_on(slack.probe(http, &delivery));
        http.finish().unwrap();
        probes
    }

    #[test]
    fn a_bot_token_is_held_to_the_scopes_its_route_needs() {
        assert_eq!(
            probed(&slack(None), &answered(200, None), false),
            [Probe::Taken]
        );
        assert_eq!(
            probed(&slack(None), &answered(200, None), true),
            [
                Probe::Taken,
                Probe::Route {
                    key: None,
                    why: "the bot token lacks the scopes pins:write; add them to the app's bot token scopes and install it again".to_owned(),
                },
            ]
        );
        let mut bare =
            Fixture::from_json(include_str!("../fixtures/probe.reference.json")).unwrap();
        if let Some(exchange) = bare.exchanges.first_mut() {
            exchange.response.headers.clear();
        }
        assert_eq!(
            probed(&slack(None), &Replay::of(bare), false),
            [
                Probe::Taken,
                Probe::Route {
                    key: None,
                    why: "the bot token lacks the scopes chat:write, users:read, channels:history or groups:history; add them to the app's bot token scopes and install it again".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_refused_token_or_an_org_wide_one_with_no_team_is_told_so() {
        assert_eq!(
            probed(
                &slack(None),
                &answered(200, Some(json!({ "ok": false, "error": "invalid_auth" }))),
                false
            ),
            [Probe::Credential(
                "Slack refuses its credential: invalid_auth".to_owned()
            )]
        );
        assert_eq!(
            probed(&slack(None), &answered(200, Some(json!({}))), false),
            [Probe::Credential(
                "Slack refuses its credential: an unnamed error".to_owned()
            )]
        );
        assert_eq!(
            probed(&slack(None), &answered(503, None), false),
            [Probe::Credential(
                "Slack answered 503 when asked whether it takes its credential".to_owned()
            )]
        );
        let grid = json!({ "ok": true, "is_enterprise_install": true });
        assert_eq!(
            probed(&slack(None), &answered(200, Some(grid)), false),
            [Probe::Credential("the token is installed across an Enterprise Grid organization, so each call names a workspace; set team to the workspace's id".to_owned())]
        );
        let mut fixture =
            Fixture::from_json(include_str!("../fixtures/probe.reference.json")).unwrap();
        if let Some(exchange) = fixture.exchanges.first_mut() {
            exchange.request.url = "https://slack.com/api/auth.test?team_id=T0123456789".to_owned();
            exchange.response.body = Some(json!({ "ok": true, "is_enterprise_install": true }));
            exchange.response.headers.clear();
            exchange.response.headers.insert(
                "x-oauth-scopes".to_owned(),
                "chat:write,channels:history,users:read".to_owned(),
            );
        }
        assert_eq!(
            probed(&slack(Some("T0123456789")), &Replay::of(fixture), false),
            [Probe::Taken]
        );
    }

    fn hooked(status: u16, said: &str) -> Vec<Probe> {
        let webhook = Slack::new(
            Credential::Webhook(Secret::new(
                "HERALD_SLACK_WEBHOOK",
                "https://hooks.slack.com/services/T0/B0/xyz",
            )),
            RouteOptions::default(),
            None,
        );
        let mut fixture =
            Fixture::from_json(include_str!("../fixtures/probe-webhook.reference.json")).unwrap();
        if let Some(exchange) = fixture.exchanges.first_mut() {
            exchange.response.status = status;
            exchange.response.body = Some(Value::String(said.to_owned()));
        }
        probed(&webhook, &Replay::of(fixture), false)
    }

    #[test]
    fn an_incoming_webhook_is_asked_with_an_empty_post_slack_refuses() {
        assert_eq!(hooked(400, "invalid_payload"), [Probe::Taken]);
        for (status, said) in [
            (403, "invalid_token"),
            (404, "no_service"),
            (410, "channel_is_archived"),
        ] {
            assert_eq!(
                hooked(status, said),
                [Probe::Credential(format!(
                    "Slack refuses its webhook, answering {status}: {said}"
                ))]
            );
        }
        assert_eq!(
            hooked(500, "oops"),
            [Probe::Credential(
                "Slack answered 500 when asked whether it takes its credential".to_owned()
            )]
        );
    }
}
