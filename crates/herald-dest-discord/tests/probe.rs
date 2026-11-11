//! What Discord says of a destination when doctor asks, against its
//! fixtures.

#[cfg(test)]
mod probe {
    use herald_core::Delivery;
    use herald_dest_discord::{Credential, Discord, RouteOptions};
    use herald_kinds::{Probe, Reach};
    use herald_ports::Secret;
    use herald_testing::{Fixture, Replay};

    macro_rules! fixture {
        ($name:literal) => {
            Replay::of(
                Fixture::from_json(include_str!(concat!(
                    "../fixtures/",
                    $name,
                    ".reference.json"
                )))
                .unwrap(),
            )
        };
    }

    fn reached(credential: Credential, channel: Option<&str>) -> Discord {
        Discord::new(
            credential,
            RouteOptions {
                channel: channel.map(ToOwned::to_owned),
                forum: false,
                subscribe: None,
            },
            None,
        )
    }

    fn bot() -> Credential {
        Credential::Bot(Secret::new("HERALD_DISCORD_TOKEN", "test-token"))
    }

    #[test]
    fn a_bot_that_cannot_see_its_route_s_channel_is_told_so() {
        let http = fixture!("probe-bot");
        let discord = reached(bot(), Some("1566184281406177531"));
        let probes = pollster::block_on(discord.probe(&http, &Delivery::default()));
        http.finish().unwrap();
        assert_eq!(
            probes,
            [
                Probe::Taken,
                Probe::Route {
                    key: Some("channel"),
                    why: "the bot cannot see the channel 1566184281406177531, as Discord answered 403; add the bot to the server and give it View Channel and Send Messages there".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_webhook_is_asked_for_itself_and_nothing_else() {
        let http = fixture!("probe-webhook");
        let webhook = Credential::Webhook(Secret::new(
            "HERALD_DISCORD_WEBHOOK",
            "https://discord.com/api/webhooks/1/hook",
        ));
        let probes = pollster::block_on(
            reached(webhook, Some("1566184281406177531")).probe(&http, &Delivery::default()),
        );
        http.finish().unwrap();
        assert_eq!(
            probes,
            [Probe::Credential(
                "Discord refuses its credential, answering 401".to_owned()
            )]
        );
    }

    fn first_answered(status: u16) -> Replay {
        let mut fixture =
            Fixture::from_json(include_str!("../fixtures/probe-bot.reference.json")).unwrap();
        fixture.exchanges.truncate(1);
        if let Some(exchange) = fixture.exchanges.first_mut() {
            exchange.response.status = status;
        }
        Replay::of(fixture)
    }

    #[test]
    fn a_refused_bot_is_not_asked_for_its_channel_nor_one_with_none() {
        let refused = first_answered(401);
        let probes = pollster::block_on(
            reached(bot(), Some("1566184281406177531")).probe(&refused, &Delivery::default()),
        );
        refused.finish().unwrap();
        assert_eq!(
            probes,
            [Probe::Credential(
                "Discord refuses its credential, answering 401".to_owned()
            )]
        );
        let taken = first_answered(200);
        let probes = pollster::block_on(reached(bot(), None).probe(&taken, &Delivery::default()));
        taken.finish().unwrap();
        assert_eq!(probes, [Probe::Taken]);
    }
}
