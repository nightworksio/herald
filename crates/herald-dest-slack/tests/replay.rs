//! Slack against its fixtures: each step's request as the fixture holds
//! it, and each answer read back into what the ledger records.

#[cfg(test)]
mod replay {
    use herald_core::{
        Announcement, Dialect, Entry, Facts, MentionStyle, Pins, ReadBack, Sent, Step, Text,
        Threads,
    };
    use herald_dest_slack::{
        Credential, DestinationOptions, Domain, Interacting, Interactions, Refusal, RouteOptions,
        Slack, SlackMarkers, profile,
    };
    use herald_ports::{Markers, MarkersError, Secret};
    use herald_testing::sample::{self, marker, route_marker};
    use herald_testing::{Fixture, Replay};
    use jiff::SignedDuration;

    const CHANNEL: &str = "C0123456789";
    const POST: &str = "1793448000.000100";
    const REPLY: &str = "1793448000.000200";
    const OLD: &str = "1793000000.000100";

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

    fn bot() -> Slack {
        Slack::new(
            Credential::Bot(Secret::new("HERALD_SLACK_TOKEN", "xoxb-test")),
            RouteOptions {
                channel: Some(CHANNEL.to_owned()),
                subscribe: None,
            },
            None,
        )
    }

    fn hook() -> Slack {
        Slack::new(
            Credential::Webhook(Secret::new(
                "HERALD_SLACK_WEBHOOK",
                "https://hooks.slack.com/services/T0/B0/xyz",
            )),
            RouteOptions::default(),
            None,
        )
    }

    fn announcement() -> Announcement {
        let mut announcement = sample::announcement();
        announcement.parts = vec![
            Text {
                body: "*Added*".to_owned(),
                html: None,
            },
            Text {
                body: "• the gate".to_owned(),
                html: None,
            },
        ];
        announcement.mentioned = vec!["U0123ABCD".to_owned(), "ana".to_owned()];
        announcement.replaces = Some(OLD.to_owned());
        announcement
    }

    fn message(ts: &str) -> Facts {
        Facts {
            message: Some(ts.to_owned()),
            ..Facts::default()
        }
    }

    fn posted() -> Entry {
        Entry::new(announcement().key).landed(Step::Post, message(POST))
    }

    fn send(
        slack: &Slack,
        replay: &Replay,
        step: &Step,
        announcement: &Announcement,
        entry: &Entry,
    ) -> Sent {
        pollster::block_on(slack.send(replay, step, announcement, entry)).unwrap()
    }

    fn refused(said: &str) -> Sent {
        Sent::Refused(said.to_owned())
    }

    #[test]
    fn a_bot_posts_its_message_threads_its_overflow_and_pins_it_in_place_of_the_earlier_pin() {
        let replay = fixture!("announcement");
        let announcement = announcement();
        let bare = Entry::new(announcement.key);
        assert_eq!(
            send(&bot(), &replay, &Step::Post, &announcement, &bare),
            Sent::Landed(Facts {
                message: Some(POST.to_owned()),
                thread: None,
                permalink: Some(
                    "https://nightworks.slack.com/archives/C0123456789/p1793448000000100"
                        .to_owned()
                ),
            })
        );
        assert_eq!(
            send(&bot(), &replay, &Step::Chunk(1), &announcement, &posted()),
            Sent::Landed(message(REPLY))
        );
        for step in [Step::Pin, Step::Unpin] {
            assert_eq!(
                send(&bot(), &replay, &step, &announcement, &posted()),
                Sent::Landed(Facts::default()),
                "{step:?}"
            );
        }
        replay.finish().unwrap();
        assert_eq!(bot().api(), "https://slack.com/api");
    }

    #[test]
    fn pins_already_as_a_step_leaves_them_land_and_a_post_without_a_permalink_still_lands() {
        let replay = fixture!("pins-in-place");
        let announcement = announcement();
        let landed = Sent::Landed(Facts::default());
        for step in [Step::Pin, Step::Unpin, Step::Unpin] {
            assert_eq!(
                send(&bot(), &replay, &step, &announcement, &posted()),
                landed
            );
        }
        assert_eq!(
            send(&bot(), &replay, &Step::Pin, &announcement, &posted()),
            refused("Slack refused it: not_in_channel")
        );
        assert_eq!(
            send(&bot(), &replay, &Step::Unpin, &announcement, &posted()),
            refused("Slack refused it: channel_not_found")
        );
        assert_eq!(
            send(
                &bot(),
                &replay,
                &Step::Post,
                &announcement,
                &Entry::new(announcement.key)
            ),
            Sent::Landed(message(POST))
        );
        replay.finish().unwrap();
    }

    #[test]
    fn a_webhook_only_posts_and_names_what_it_cannot_do() {
        let replay = fixture!("webhook");
        let announcement = announcement();
        let bare = Entry::new(announcement.key);
        let landed = Sent::Landed(Facts::default());
        assert_eq!(
            send(&hook(), &replay, &Step::Post, &announcement, &bare),
            landed
        );
        assert_eq!(
            send(&hook(), &replay, &Step::Chunk(1), &announcement, &posted()),
            landed
        );
        for said in [
            "Slack refused it with 400: invalid_payload",
            "Slack refused it with 404: no_service",
            "Slack refused it with 410",
        ] {
            assert_eq!(
                send(&hook(), &replay, &Step::Post, &announcement, &bare),
                refused(said)
            );
        }
        for (step, what) in [(Step::Pin, "pin"), (Step::Unpin, "unpin")] {
            assert_eq!(
                send(&hook(), &replay, &step, &announcement, &posted()),
                refused(&format!(
                    "a Slack webhook cannot {what}; reach Slack with a bot token to"
                ))
            );
        }
        replay.finish().unwrap();
        assert_eq!(hook().api(), "https://hooks.slack.com");
        assert_eq!(
            Slack::new(
                Credential::Bot(Secret::new("T", "t")),
                RouteOptions::default(),
                Some("http://127.0.0.1:8080/api/")
            )
            .api(),
            "http://127.0.0.1:8080/api"
        );
    }

    #[test]
    fn a_limit_or_an_outage_is_asked_again_after_the_wait_it_names_or_its_own() {
        let replay = fixture!("rate-limited");
        let announcement = announcement();
        let bare = Entry::new(announcement.key);
        for wait in [7, 5, 5, 30] {
            assert_eq!(
                send(&bot(), &replay, &Step::Post, &announcement, &bare),
                Sent::RetryAfter(SignedDuration::from_secs(wait))
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn a_refusal_names_slack_s_error_and_the_scope_the_token_lacks() {
        let replay = fixture!("refused");
        let announcement = announcement();
        let bare = Entry::new(announcement.key);
        for said in [
            "Slack refused it: channel_not_found",
            "Slack refused it: missing_scope, as the token lacks chat:write",
            "Slack refused it: an unnamed error",
            "Slack's answer holds no message timestamp",
            "Slack refused it with 403: invalid_auth",
        ] {
            assert_eq!(
                send(&bot(), &replay, &Step::Post, &announcement, &bare),
                refused(said)
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn an_unpin_with_no_remembered_pin_unpins_herald_s_own_pins_of_the_route_and_no_others() {
        let replay = fixture!("unpin-by-marker");
        let mut unremembered = announcement();
        unremembered.replaces = None;
        let unpin = || send(&bot(), &replay, &Step::Unpin, &unremembered, &posted());
        assert_eq!(unpin(), Sent::Landed(Facts::default()));
        assert_eq!(unpin(), Sent::RetryAfter(SignedDuration::from_secs(3)));
        assert_eq!(unpin(), refused("Slack refused it: invalid_auth"));
        assert_eq!(
            unpin(),
            refused("Slack did not say which bot the token is; give a bot token (xoxb-…)")
        );
        assert_eq!(
            unpin(),
            refused("Slack did not say which app the bot belongs to")
        );
        assert_eq!(unpin(), refused("Slack refused it: bot_not_found"));
        assert_eq!(unpin(), Sent::RetryAfter(SignedDuration::from_secs(5)));
        assert_eq!(unpin(), refused("Slack refused it: not_in_channel"));
        replay.finish().unwrap();
    }

    #[test]
    fn a_step_needing_an_earlier_one_or_a_thing_slack_has_not_is_refused_unsent() {
        let replay = Replay::of(Fixture::default());
        let announcement = announcement();
        let bare = Entry::new(announcement.key);
        let unchannelled = Slack::new(
            Credential::Bot(Secret::new("HERALD_SLACK_TOKEN", "xoxb-test")),
            RouteOptions::default(),
            None,
        );
        let not_yet =
            "the announcement's message is not recorded, so this step cannot be taken yet";
        for (slack, step, entry, said) in [
            (bot(), Step::Chunk(1), &bare, not_yet),
            (bot(), Step::Pin, &bare, not_yet),
            (bot(), Step::Unpin, &bare, not_yet),
            (bot(), Step::Crosspost, &bare, "Slack cannot crosspost"),
            (
                bot(),
                Step::OpenThread,
                &bare,
                "Slack cannot open a thread; its replies are the thread",
            ),
            (
                hook(),
                Step::Deliver("https://a.example/inbox".to_owned()),
                &bare,
                "Slack cannot deliver to an inbox",
            ),
            (
                unchannelled.clone(),
                Step::Post,
                &bare,
                "a Slack bot posts to a channel, and the route names none",
            ),
            (
                unchannelled,
                Step::Unpin,
                &posted(),
                "a Slack bot posts to a channel, and the route names none",
            ),
        ] {
            assert_eq!(
                send(&slack, &replay, &step, &announcement, entry),
                refused(said),
                "{step:?}"
            );
        }
        replay.finish().unwrap();
        assert_eq!(
            Refusal::Missing("the route's earlier pin").to_string(),
            "the route's earlier pin is not recorded, so this step cannot be taken yet"
        );
    }

    fn body(slack: &Slack, announcement: &Announcement) -> serde_json::Value {
        let request = slack
            .request(&Step::Post, announcement, &Entry::new(announcement.key))
            .unwrap();
        serde_json::from_slice(&request.body).unwrap()
    }

    #[test]
    fn mentions_past_the_announcement_s_room_go_in_messages_after_its_parts() {
        let mut many = announcement();
        many.mentioned_after = vec![vec!["U0123ABCD".to_owned(), "ana".to_owned()]];
        let after = u32::try_from(many.parts.len()).unwrap();
        let posted = Entry::new(many.key).landed(
            Step::Post,
            herald_core::Facts {
                message: Some("1700000000.000100".to_owned()),
                ..herald_core::Facts::default()
            },
        );
        let request = bot().request(&Step::Chunk(after), &many, &posted).unwrap();
        let content: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(content["text"], "<@U0123ABCD>");
        assert_eq!(content["blocks"][0]["text"]["text"], "<@U0123ABCD>");
        assert_eq!(many.messages(), many.parts.len() + 1);
    }

    #[test]
    fn only_a_user_written_as_a_slack_user_id_is_mentioned_and_a_link_is_read_whole() {
        let mut mentioning = announcement();
        mentioning.mentioned = [
            "U0123ABCD",
            "W0123ABCD",
            "u0123abcd",
            "U0",
            "U01<!>",
            "C0123",
            "ana",
        ]
        .map(ToOwned::to_owned)
        .to_vec();
        mentioning.link = Some("https://example.org/a?b=1&c=<x>|y".to_owned());
        let content = body(&bot(), &mentioning);
        let blocks = content["blocks"].as_array().unwrap();
        assert_eq!(blocks[3]["text"]["text"], "<@U0123ABCD> <@W0123ABCD>");
        assert_eq!(
            blocks[2]["text"]["text"],
            "<https://example.org/a?b=1&amp;c=%3Cx%3E%7Cy>"
        );
        let mut quiet = announcement();
        quiet.mentioned = Vec::new();
        quiet.link = None;
        quiet.parts[0].body = String::new();
        quiet.title = herald_core::Title::new("a <b> & c");
        let content = body(&bot(), &quiet);
        assert_eq!(content["blocks"].as_array().unwrap().len(), 1);
        assert_eq!(content["blocks"][0]["text"]["text"], "a <b> & c");
        assert_eq!(content["text"], "a &lt;b&gt; &amp; c");
    }

    #[test]
    fn its_own_marked_message_its_thread_and_its_pin_are_read_back_and_no_one_elses() {
        let slack = bot();
        let replay = fixture!("markers");
        let markers = SlackMarkers::new(&slack, &replay);
        let key = announcement().key;
        let read = || pollster::block_on(markers.entry(&key, &marker())).unwrap();
        assert_eq!(
            read(),
            Some(
                Entry::new(key)
                    .landed(Step::Post, message(POST))
                    .landed(Step::Chunk(1), message(REPLY))
                    .landed(Step::Chunk(2), message("1793448000.000300"))
                    .landed(Step::Pin, Facts::default())
                    .landed(Step::Unpin, Facts::default())
            )
        );
        assert_eq!(
            read(),
            Some(Entry::new(key).landed(Step::Post, message(POST)))
        );
        assert_eq!(read(), None);
        assert_eq!(
            pollster::block_on(markers.threads(&route_marker())).unwrap(),
            []
        );
        replay.finish().unwrap();
    }

    #[test]
    fn a_marked_message_without_a_timestamp_is_no_entry_and_a_channel_is_asked_for_encoded() {
        let slack = bot();
        let replay = fixture!("markers-edges");
        let key = announcement().key;
        assert_eq!(
            pollster::block_on(SlackMarkers::new(&slack, &replay).entry(&key, &marker())).unwrap(),
            None
        );
        let spaced = Slack::new(
            Credential::Bot(Secret::new("HERALD_SLACK_TOKEN", "xoxb-test")),
            RouteOptions {
                channel: Some("C0 1".to_owned()),
                subscribe: None,
            },
            None,
        );
        assert_eq!(
            pollster::block_on(SlackMarkers::new(&spaced, &replay).entry(&key, &marker())).unwrap(),
            None
        );
        replay.finish().unwrap();
    }

    #[test]
    fn a_request_that_never_got_an_answer_leaves_the_channel_unread() {
        let slack = bot();
        let key = announcement().key;
        let silent = Replay::of(Fixture::default());
        let identity =
            pollster::block_on(SlackMarkers::new(&slack, &silent).entry(&key, &marker()));
        assert!(
            matches!(identity, Err(MarkersError::Unreadable { .. })),
            "{identity:?}"
        );
        let cut_short = fixture!("markers-cut-short");
        let history =
            pollster::block_on(SlackMarkers::new(&slack, &cut_short).entry(&key, &marker()));
        assert!(
            matches!(history, Err(MarkersError::Unreadable { .. })),
            "{history:?}"
        );
    }

    #[test]
    fn history_and_replies_are_read_back_page_by_page_until_the_post_and_no_further_than_read_back()
    {
        let slack = bot();
        let replay = fixture!("markers-paged");
        let key = announcement().key;
        assert_eq!(
            pollster::block_on(SlackMarkers::new(&slack, &replay).entry(&key, &marker())).unwrap(),
            Some(
                Entry::new(key)
                    .landed(Step::Post, message(POST))
                    .landed(Step::Chunk(1), message(REPLY))
            )
        );
        let one_page = slack.reading_back(ReadBack::new(1).unwrap());
        assert_eq!(
            pollster::block_on(SlackMarkers::new(&one_page, &replay).entry(&key, &marker()))
                .unwrap(),
            None
        );
        replay.finish().unwrap();
    }

    #[test]
    fn a_domain_places_the_api_its_webhooks_and_its_sessions_unless_an_api_is_given() {
        let token = || Credential::Bot(Secret::new("HERALD_SLACK_TOKEN", "xoxb-test"));
        let hook = || {
            Credential::Webhook(Secret::new(
                "HERALD_SLACK_WEBHOOK",
                "https://hooks.slack-gov.com/services/T/B/x",
            ))
        };
        let route = RouteOptions::default;
        assert_eq!(
            Slack::new(token(), route(), None).api(),
            "https://slack.com/api"
        );
        assert_eq!(
            Slack::new(hook(), route(), None).api(),
            "https://hooks.slack.com"
        );
        assert_eq!(
            Slack::new(token(), route(), None).on(Domain::Gov).api(),
            "https://slack-gov.com/api"
        );
        assert_eq!(
            Slack::new(hook(), route(), None).on(Domain::Gov).api(),
            "https://hooks.slack-gov.com"
        );
        assert_eq!(
            Slack::new(token(), route(), Some("http://127.0.0.1:9/"))
                .on(Domain::Gov)
                .api(),
            "http://127.0.0.1:9"
        );
        let interactions: Interactions = serde_json::from_value(serde_json::json!({
            "via": "socket",
            "app": { "secret": "HERALD_SLACK_APP" },
            "domains": ["proxy.example"],
        }))
        .unwrap();
        let gov = Interacting::new(interactions.clone(), None).on(Domain::Gov);
        assert_eq!(
            (gov.api(), gov.hooks()),
            ("https://slack-gov.com/api", "https://hooks.slack-gov.com")
        );
        assert_eq!(gov.session_domains(), ["slack-gov.com", "proxy.example"]);
        let given = Interacting::new(interactions, Some("http://127.0.0.1:9")).on(Domain::Gov);
        assert_eq!(
            (given.api(), given.hooks()),
            ("http://127.0.0.1:9", "http://127.0.0.1:9")
        );
    }

    #[test]
    fn an_org_wide_app_names_its_workspace_in_each_web_api_call_and_no_other_app_does() {
        let announcement = announcement();
        let fresh = Entry::new(announcement.key);
        let team = |slack: &Slack| {
            let post: serde_json::Value = serde_json::from_slice(
                &slack
                    .request(&Step::Post, &announcement, &fresh)
                    .unwrap()
                    .body,
            )
            .unwrap();
            let pins = slack.request(&Step::Unpin, &announcement, &fresh).unwrap();
            (post.get("team_id").cloned(), pins.url.redacted())
        };
        assert_eq!(
            team(&bot().in_team(Some("T0123456789"))),
            (
                Some(serde_json::json!("T0123456789")),
                format!("https://slack.com/api/pins.list?channel={CHANNEL}&team_id=T0123456789")
            )
        );
        assert_eq!(
            team(&bot()),
            (
                None,
                format!("https://slack.com/api/pins.list?channel={CHANNEL}")
            )
        );
    }

    #[test]
    fn a_channel_that_cannot_be_read_says_why() {
        let slack = bot();
        let replay = fixture!("markers-unreadable");
        let markers = SlackMarkers::new(&slack, &replay);
        let key = announcement().key;
        for reason in [
            "Slack refused it: invalid_auth",
            "Slack asked to wait 3s first",
            "Slack refused it: thread_not_found",
        ] {
            assert_eq!(
                pollster::block_on(markers.entry(&key, &marker())),
                Err(MarkersError::Unreadable {
                    reason: reason.to_owned()
                })
            );
        }
        replay.finish().unwrap();
        let none = Replay::of(Fixture::default());
        let unchannelled = Slack::new(
            Credential::Bot(Secret::new("HERALD_SLACK_TOKEN", "xoxb-test")),
            RouteOptions::default(),
            None,
        );
        for (slack, reason) in [
            (hook(), "a webhook cannot read a channel's messages"),
            (
                unchannelled,
                "a Slack bot posts to a channel, and the route names none",
            ),
        ] {
            assert_eq!(
                pollster::block_on(SlackMarkers::new(&slack, &none).entry(&key, &marker())),
                Err(MarkersError::Unreadable {
                    reason: reason.to_owned()
                })
            );
        }
        none.finish().unwrap();
    }

    #[test]
    fn a_destination_takes_one_credential_a_channel_is_named_by_its_id_and_slack_does_what_its_profile_says()
     {
        let secret = || {
            Some(herald_core::SecretRef {
                secret: "S".to_owned(),
            })
        };
        let reached = |token, webhook| DestinationOptions {
            token,
            webhook,
            api: None,
            domain: None,
            team: None,
            interactions: None,
            read_back: None,
        };
        assert_eq!(reached(secret(), None).bot(), Ok(true));
        assert_eq!(reached(None, secret()).bot(), Ok(false));
        assert_eq!(
            reached(None, None).bot(),
            Err("a Slack destination needs a token or a webhook; give one")
        );
        assert_eq!(
            reached(secret(), secret()).bot(),
            Err("a Slack destination takes a token or a webhook, not both; keep one")
        );
        for channel in ["C0123456789", "G01234567", "D0123ABCD"] {
            assert_eq!(
                RouteOptions {
                    channel: Some(channel.to_owned()),
                    subscribe: None,
                }
                .check(),
                Ok(()),
                "{channel}"
            );
        }
        assert_eq!(RouteOptions::default().check(), Ok(()));
        for channel in [
            "#releases",
            "C0123456",
            "c0123456789",
            "C01234567x9",
            "X0123456789",
        ] {
            assert_eq!(
                RouteOptions {
                    channel: Some(channel.to_owned()),
                    subscribe: None,
                }
                .check(),
                Err(format!(
                    "\"{channel}\" is not a channel id; give the channel's id, such as \"C0123456789\", from its details"
                )),
                "{channel}"
            );
        }
        let as_bot = profile(true);
        assert_eq!(bot().profile(), as_bot);
        assert_eq!(hook().profile(), profile(false));
        assert_eq!(as_bot.dialect, Dialect::Slack);
        assert_eq!(as_bot.features.threads, Threads::Replies);
        assert_eq!(as_bot.features.pins, Pins::Many);
        assert!(matches!(as_bot.features.mentions, MentionStyle::Users(_)));
        assert!(
            as_bot.features.reads_back && as_bot.features.edits && as_bot.features.interactions
        );
        assert!(!as_bot.features.crosspost);
        let as_hook = profile(false);
        assert_eq!(as_hook.features.threads, Threads::None);
        assert_eq!(as_hook.features.pins, Pins::None);
        assert!(
            !as_hook.features.reads_back
                && !as_hook.features.edits
                && !as_hook.features.interactions
        );
        assert_eq!(
            (
                as_bot.limits.title,
                as_bot.limits.text,
                as_bot.limits.followup,
                as_bot.limits.thread_name
            ),
            (150, 3_000, 3_000, 150)
        );
    }
}

#[cfg(test)]
mod subscribing {
    use herald_core::{Entry, Step};
    use herald_dest_slack::{Credential, RouteOptions, Slack, Subscribe, subscribe_block};
    use herald_ports::Secret;
    use herald_testing::sample;
    use serde_json::Value;

    fn posted(credential: Credential, subscribe: Option<Subscribe>) -> Value {
        let slack = Slack::new(
            credential,
            RouteOptions {
                channel: Some("C0123456789".to_owned()),
                subscribe,
            },
            None,
        );
        let announcement = sample::announcement();
        let request = slack
            .request(&Step::Post, &announcement, &Entry::new(announcement.key))
            .unwrap();
        serde_json::from_slice(&request.body).unwrap()
    }

    fn bot() -> Credential {
        Credential::Bot(Secret::new("HERALD_SLACK_TOKEN", "xoxb-test"))
    }

    #[test]
    fn an_announcement_ends_with_a_subscribe_button_where_its_route_offers_one() {
        let hook = Credential::Webhook(Secret::new(
            "HERALD_SLACK_WEBHOOK",
            "https://hooks.slack.com/services/T0/B0/xyz",
        ));
        for credential in [bot(), hook] {
            let body = posted(credential, Some(Subscribe { button: true }));
            let blocks = body["blocks"].as_array().unwrap();
            assert_eq!(blocks.last(), Some(&subscribe_block("php-mutation-gate")));
        }
        for quiet in [Some(Subscribe { button: false }), None] {
            let body = posted(bot(), quiet);
            let blocks = body["blocks"].as_array().unwrap();
            assert!(blocks.iter().all(|block| block["type"] != "actions"));
        }
    }
}
