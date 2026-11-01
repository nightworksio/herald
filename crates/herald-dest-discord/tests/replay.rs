//! Discord against its fixtures: each step's request as the fixture holds
//! it, and each answer read back into what the ledger records.

#[cfg(test)]
mod replay {
    use herald_core::{
        Announcement, Chore, Delivery, Entry, Facts, Marker, Pins, Plan, Planned, Sent, Step, plan,
    };
    use herald_dest_discord::{
        Credential, DestinationOptions, Discord, DiscordMarkers, Refusal, RouteOptions,
    };
    use herald_ports::{Markers, MarkersError, Secret};
    use herald_testing::sample::{self, announcement, marker};
    use herald_testing::{Fixture, Replay};
    use jiff::SignedDuration;

    const CHANNEL: &str = "1566184281406177531";
    const FORUM: &str = "1566184281406177600";

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

    fn bot(channel: &str, forum: bool) -> Discord {
        Discord::new(
            Credential::Bot(Secret::new("HERALD_DISCORD_TOKEN", "test-token")),
            RouteOptions {
                channel: Some(channel.to_owned()),
                forum,
            },
            None,
        )
    }

    fn webhook(forum: bool) -> Discord {
        Discord::new(
            Credential::Webhook(Secret::new(
                "HERALD_DISCORD_WEBHOOK",
                "https://discord.com/api/webhooks/1/hook",
            )),
            RouteOptions {
                channel: None,
                forum,
            },
            None,
        )
    }

    const ROLE: &str = "1566190000000000001";

    fn delivery() -> Delivery {
        sample::delivery(ROLE)
    }

    fn landed(facts: Facts) -> Sent {
        Sent::Landed(facts)
    }

    fn message(id: &str) -> Facts {
        Facts {
            message: Some(id.to_owned()),
            ..Facts::default()
        }
    }

    fn send(discord: &Discord, replay: &Replay, step: &Step, entry: &Entry) -> Sent {
        pollster::block_on(discord.send(replay, step, &announcement(), &delivery(), entry)).unwrap()
    }

    #[test]
    fn a_bot_posts_crossposts_threads_chunks_and_pins_an_announcement() {
        let discord = bot(CHANNEL, false);
        let replay = fixture!("announcement");
        let mut entry = Entry::new(announcement().key);
        let steps = [
            (Step::Post, message("1566200000000000001")),
            (Step::Crosspost, message("1566200000000000001")),
            (
                Step::OpenThread,
                Facts {
                    thread: Some("1566200000000000001".to_owned()),
                    ..Facts::default()
                },
            ),
            (Step::Chunk(1), message("1566200000000000002")),
            (Step::Pin, Facts::default()),
            (Step::Unpin, Facts::default()),
        ];
        for (step, facts) in steps {
            assert_eq!(
                send(&discord, &replay, &step, &entry),
                landed(facts.clone()),
                "{step:?}"
            );
            entry = entry.landed(step, facts);
        }
        replay.finish().unwrap();
    }

    #[test]
    fn a_bot_posts_into_a_forum_and_chunks_into_the_post() {
        let discord = bot(FORUM, true);
        let replay = fixture!("forum");
        let facts = Facts {
            message: Some("1566200000000000010".to_owned()),
            thread: Some("1566200000000000010".to_owned()),
            permalink: None,
        };
        let entry = Entry::new(announcement().key);
        assert_eq!(
            send(&discord, &replay, &Step::Post, &entry),
            landed(facts.clone())
        );
        let entry = entry.landed(Step::Post, facts);
        assert_eq!(
            send(&discord, &replay, &Step::Chunk(1), &entry),
            landed(message("1566200000000000011"))
        );
        for step in [Step::Unpin, Step::Pin] {
            assert_eq!(
                send(&discord, &replay, &step, &entry),
                landed(Facts::default()),
                "{step:?}"
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn a_forum_whose_pinned_posts_cannot_be_read_says_why() {
        let replay = fixture!("forum-pin-unreadable");
        let discord = bot(FORUM, true);
        let entry = Entry::new(announcement().key).landed(
            Step::Post,
            Facts {
                message: Some("1566200000000000010".to_owned()),
                thread: Some("1566200000000000010".to_owned()),
                permalink: None,
            },
        );
        for _ in 0..3 {
            assert_eq!(
                unpin(&discord, &replay, &announcement(), &entry),
                Sent::Refused("Discord refused it with 403: 50001 Missing Access".to_owned())
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn a_forum_unpins_the_post_the_store_remembers_by_its_flag() {
        let request = bot(FORUM, true)
            .request(
                &Step::Unpin,
                &replacing("1566200000000000018"),
                &delivery(),
                &pinned(),
            )
            .unwrap();
        assert_eq!(
            request.url.redacted(),
            "https://discord.com/api/v10/channels/1566200000000000018"
        );
        assert_eq!(request.body, br#"{"flags":0}"#);
    }

    #[test]
    fn a_webhook_posts_and_chunks() {
        let discord = webhook(false);
        let replay = fixture!("webhook");
        let entry = Entry::new(announcement().key);
        assert_eq!(
            send(&discord, &replay, &Step::Post, &entry),
            landed(message("1566200000000000020"))
        );
        let entry = entry.landed(Step::Post, message("1566200000000000020"));
        assert_eq!(
            send(&discord, &replay, &Step::Chunk(1), &entry),
            landed(message("1566200000000000021"))
        );
        replay.finish().unwrap();
    }

    #[test]
    fn a_webhook_posts_into_a_forum_and_chunks_into_the_post() {
        let discord = webhook(true);
        let replay = fixture!("webhook-forum");
        let facts = Facts {
            message: Some("1566200000000000030".to_owned()),
            thread: Some("1566200000000000030".to_owned()),
            permalink: None,
        };
        let entry = Entry::new(announcement().key);
        assert_eq!(
            send(&discord, &replay, &Step::Post, &entry),
            landed(facts.clone())
        );
        let entry = entry.landed(Step::Post, facts);
        assert_eq!(
            send(&discord, &replay, &Step::Chunk(1), &entry),
            landed(message("1566200000000000031"))
        );
        replay.finish().unwrap();
    }

    #[test]
    fn what_a_destination_cannot_do_is_refused_before_anything_is_sent() {
        let replay = Replay::of(Fixture::default());
        let entry = Entry::new(announcement().key);
        let refused = |discord: &Discord, step: Step| match send(discord, &replay, &step, &entry) {
            Sent::Refused(reason) => reason,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            refused(&webhook(false), Step::Crosspost),
            "a Discord webhook cannot crosspost; use a bot token"
        );
        assert_eq!(
            refused(&webhook(false), Step::OpenThread),
            "a Discord webhook cannot open a thread from a message; use a bot token"
        );
        assert_eq!(
            refused(&webhook(false), Step::Pin),
            "a Discord webhook cannot pin; use a bot token"
        );
        assert_eq!(
            refused(&webhook(false), Step::Deliver("x".to_owned())),
            "Discord delivers to no inbox"
        );
        assert_eq!(
            refused(&bot(CHANNEL, false), Step::Deliver("x".to_owned())),
            "Discord delivers to no inbox"
        );
        let nowhere = Discord::new(
            Credential::Bot(Secret::new("T", "t")),
            RouteOptions::default(),
            None,
        );
        assert_eq!(
            refused(&nowhere, Step::Post),
            "the route names no Discord channel; set its channel"
        );
        assert_eq!(
            refused(&nowhere, Step::Unpin),
            "the route names no Discord channel; set its channel"
        );
        assert_eq!(
            refused(&webhook(false), Step::Unpin),
            "a Discord webhook cannot unpin; use a bot token"
        );
        for step in [Step::Crosspost, Step::OpenThread, Step::Pin, Step::Unpin] {
            assert_eq!(
                refused(&bot(CHANNEL, false), step),
                "the announcement's message is not recorded, so this step cannot be taken yet"
            );
        }
        let without_id = entry.clone().landed(Step::Post, Facts::default());
        assert!(matches!(
            send(&bot(CHANNEL, false), &replay, &Step::Pin, &without_id),
            Sent::Refused(_)
        ));
        replay.finish().unwrap();
    }

    fn pinned() -> Entry {
        Entry::new(announcement().key).landed(Step::Post, message("1566200000000000001"))
    }

    fn replacing(earlier: &str) -> Announcement {
        Announcement {
            replaces: Some(earlier.to_owned()),
            ..announcement()
        }
    }

    fn unpin(
        discord: &Discord,
        replay: &Replay,
        announcement: &Announcement,
        entry: &Entry,
    ) -> Sent {
        pollster::block_on(discord.send(replay, &Step::Unpin, announcement, &delivery(), entry))
            .unwrap()
    }

    #[test]
    fn the_pin_the_store_remembers_is_unpinned_and_one_already_gone_is_done() {
        let replay = fixture!("unpin-remembered");
        let discord = bot(CHANNEL, false);
        let earlier = replacing("1566200000000000008");
        assert_eq!(
            unpin(&discord, &replay, &earlier, &pinned()),
            Sent::Landed(Facts::default())
        );
        assert_eq!(
            unpin(&discord, &replay, &earlier, &pinned()),
            Sent::Refused("Discord refused it with 403: 50013 Missing Permissions".to_owned())
        );
        let itself = replacing("1566200000000000001");
        assert_eq!(
            unpin(&discord, &replay, &itself, &pinned()),
            Sent::Landed(Facts::default())
        );
        replay.finish().unwrap();
    }

    #[test]
    fn pins_that_cannot_be_read_say_why() {
        let replay = fixture!("unpin-unreadable");
        let discord = bot(CHANNEL, false);
        let refusals = [
            "Discord did not say which user the bot is",
            "Discord refused it with 401: 0 401: Unauthorized",
            "Discord refused it with 403: 50001 Missing Access",
        ];
        for refusal in refusals {
            assert_eq!(
                unpin(&discord, &replay, &announcement(), &pinned()),
                Sent::Refused(refusal.to_owned())
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn an_unpin_is_built_for_the_pin_the_store_remembers() {
        let discord = bot(CHANNEL, false);
        let request = discord
            .request(
                &Step::Unpin,
                &replacing("1566200000000000008"),
                &delivery(),
                &pinned(),
            )
            .unwrap();
        assert_eq!(
            request.to_string(),
            "DELETE https://discord.com/api/v10/channels/1566184281406177531/messages/pins/1566200000000000008\nUser-Agent: DiscordBot (https://github.com/nightworksio/herald, 0.0.0)\nAuthorization: Bot «secret:HERALD_DISCORD_TOKEN»"
        );
        assert_eq!(
            discord.request(&Step::Unpin, &announcement(), &delivery(), &pinned()),
            Err(Refusal::Missing("the route's earlier pin"))
        );
        assert_eq!(
            webhook(false).request(&Step::Unpin, &announcement(), &delivery(), &pinned()),
            Err(Refusal::WebhookCannot("unpin"))
        );
    }

    #[test]
    fn a_rate_limit_or_an_outage_says_when_to_ask_again() {
        let entry = Entry::new(announcement().key);
        let replay = fixture!("rate-limited");
        assert_eq!(
            send(&bot(CHANNEL, false), &replay, &Step::Post, &entry),
            Sent::RetryAfter(SignedDuration::from_millis(1500))
        );
        let replay = fixture!("outage");
        assert_eq!(
            send(&bot(CHANNEL, false), &replay, &Step::Post, &entry),
            Sent::RetryAfter(SignedDuration::from_secs(30))
        );
        assert_eq!(
            send(&bot(CHANNEL, false), &replay, &Step::Post, &entry),
            Sent::RetryAfter(SignedDuration::from_secs(5))
        );
        replay.finish().unwrap();
    }

    #[test]
    fn a_refusal_says_what_discord_said() {
        let replay = fixture!("refused");
        assert_eq!(
            send(
                &bot(CHANNEL, false),
                &replay,
                &Step::Post,
                &Entry::new(announcement().key)
            ),
            Sent::Refused("Discord refused it with 403: 50013 Missing Permissions".to_owned())
        );
        replay.finish().unwrap();
    }

    #[test]
    fn housekeeping_archives_a_thread_and_locks_one_by_unarchiving_it_first() {
        let replay = fixture!("housekeeping");
        let discord = bot(CHANNEL, false);
        assert_eq!(
            pollster::block_on(discord.housekeep(&replay, Chore::Archive, "1566200000000000001"))
                .unwrap(),
            Sent::Landed(Facts::default())
        );
        assert_eq!(
            pollster::block_on(discord.housekeep(&replay, Chore::Lock, "1566200000000000001"))
                .unwrap(),
            Sent::Landed(Facts::default())
        );
        assert_eq!(
            pollster::block_on(discord.housekeep(&replay, Chore::Lock, "1566200000000000002"))
                .unwrap(),
            Sent::Refused("Discord refused it with 404: 10003 Unknown Channel".to_owned())
        );
        replay.finish().unwrap();
        assert_eq!(
            pollster::block_on(webhook(false).housekeep(
                &Replay::of(Fixture::default()),
                Chore::Lock,
                "1"
            ))
            .unwrap(),
            Sent::Refused(
                "a Discord webhook cannot archive or lock a thread; use a bot token".to_owned()
            )
        );
    }

    fn read_back(
        discord: &Discord,
        http: &Replay,
        marker: &Marker,
    ) -> Result<Option<Entry>, MarkersError> {
        pollster::block_on(DiscordMarkers::new(discord, http).entry(&announcement().key, marker))
    }

    #[test]
    fn markers_read_back_which_steps_landed() {
        let replay = fixture!("markers");
        let entry = read_back(&bot(CHANNEL, false), &replay, &marker())
            .unwrap()
            .unwrap();
        let thread = Facts {
            thread: Some("1566200000000000001".to_owned()),
            ..Facts::default()
        };
        let expected = Entry::new(announcement().key)
            .landed(Step::Post, message("1566200000000000001"))
            .landed(Step::Crosspost, message("1566200000000000001"))
            .landed(Step::Pin, Facts::default())
            .landed(Step::OpenThread, thread.clone())
            .landed(Step::Chunk(1), thread.clone())
            .landed(Step::Chunk(2), thread);
        assert_eq!(entry, expected);
        replay.finish().unwrap();
    }

    #[test]
    fn markers_without_a_thread_or_a_match_say_so() {
        let discord = bot(CHANNEL, false);
        let replay = fixture!("markers-unthreaded");
        assert_eq!(
            read_back(&discord, &replay, &marker()).unwrap(),
            Some(Entry::new(announcement().key).landed(Step::Post, message("1566200000000000001")))
        );
        let replay = fixture!("markers-unthreaded");
        assert_eq!(
            read_back(&discord, &replay, &Marker::of(&[0x5a; 32])).unwrap(),
            None
        );
    }

    #[test]
    fn a_planted_or_guessed_marker_is_not_herald_s_post_so_the_announcement_is_still_posted() {
        let replay = fixture!("markers-planted");
        let entry = read_back(&bot(CHANNEL, false), &replay, &marker()).unwrap();
        assert_eq!(entry, None);
        replay.finish().unwrap();
        let now = "2026-10-31T12:00:00Z".parse().unwrap();
        assert_eq!(
            plan(entry.as_ref(), &[Step::Post], "run", now),
            Plan::Send(vec![Planned {
                step: Step::Post,
                verify_first: false,
            }])
        );
    }

    #[test]
    fn markers_that_cannot_be_read_say_why() {
        let discord = bot(CHANNEL, false);
        let replay = fixture!("markers-unreadable");
        assert_eq!(
            read_back(&discord, &replay, &marker())
                .unwrap_err()
                .to_string(),
            "the destination's posts could not be read: Discord answered 403"
        );
        let nothing = Replay::of(Fixture::default());
        assert!(read_back(&discord, &nothing, &marker()).is_err());
        assert_eq!(
            read_back(&webhook(false), &nothing, &marker())
                .unwrap_err()
                .to_string(),
            "the destination's posts could not be read: a webhook cannot read a channel's messages"
        );
        let anonymous = Replay::of(
            Fixture::from_json(
                r#"{"exchanges":[{"request":{"method":"GET","url":"https://discord.com/api/v10/users/@me","headers":{"Authorization":"Bot «secret:HERALD_DISCORD_TOKEN»","User-Agent":"DiscordBot (https://github.com/nightworksio/herald, 0.0.0)"}},"response":{"status":200,"body":{"username":"herald"}}}]}"#,
            )
            .unwrap(),
        );
        assert_eq!(
            read_back(&discord, &anonymous, &marker())
                .unwrap_err()
                .to_string(),
            "the destination's posts could not be read: Discord did not say which user the bot is"
        );
    }

    #[test]
    fn a_thread_without_a_name_of_its_own_takes_the_title() {
        let unnamed = Announcement {
            thread: None,
            ..announcement()
        };
        let request = bot(FORUM, true)
            .request(&Step::Post, &unnamed, &delivery(), &Entry::new(unnamed.key))
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["name"], "📦 php-mutation-gate v0.1.0");
    }

    #[test]
    fn a_prerelease_is_marked_by_its_colour() {
        let early = Announcement {
            prerelease: true,
            ..announcement()
        };
        let request = bot(CHANNEL, false)
            .request(&Step::Post, &early, &delivery(), &Entry::new(early.key))
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["embeds"][0]["color"], 0x008F_88C7);
    }

    #[test]
    fn a_post_shows_a_button_for_each_link_it_has() {
        let body = |announcement: &Announcement| -> serde_json::Value {
            let request = bot(CHANNEL, false)
                .request(
                    &Step::Post,
                    announcement,
                    &delivery(),
                    &Entry::new(announcement.key),
                )
                .unwrap();
            serde_json::from_slice(&request.body).unwrap()
        };
        let unlinked = Announcement {
            release: None,
            changelog: None,
            ..announcement()
        };
        assert_eq!(body(&unlinked).get("components"), None);
        let changelog_only = Announcement {
            release: None,
            ..announcement()
        };
        let buttons = &body(&changelog_only)["components"][0]["components"];
        assert_eq!(buttons.as_array().map(Vec::len), Some(1));
        assert_eq!(buttons[0]["label"], "Changelog");
    }

    #[test]
    fn an_answer_without_an_id_is_refused() {
        let replay = fixture!("answer-without-id");
        assert_eq!(
            send(
                &bot(CHANNEL, false),
                &replay,
                &Step::Post,
                &Entry::new(announcement().key)
            ),
            Sent::Refused("Discord's answer holds no id".to_owned())
        );
        replay.finish().unwrap();
    }

    #[test]
    fn the_api_is_discord_s_own_unless_another_base_is_named() {
        assert_eq!(bot(CHANNEL, false).api(), "https://discord.com/api/v10");
        let local = Discord::new(
            Credential::Bot(Secret::new("T", "t")),
            RouteOptions::default(),
            Some("http://127.0.0.1:8080/api/"),
        );
        assert_eq!(local.api(), "http://127.0.0.1:8080/api");
    }

    #[test]
    fn a_destination_posts_as_a_bot_or_through_a_webhook_but_not_both() {
        let secret = || {
            Some(herald_core::SecretRef {
                secret: "S".to_owned(),
            })
        };
        let options = |token, webhook| DestinationOptions {
            token,
            webhook,
            api: None,
        };
        assert_eq!(options(secret(), None).bot(), Ok(true));
        assert_eq!(options(None, secret()).bot(), Ok(false));
        assert!(
            options(None, None)
                .bot()
                .unwrap_err()
                .contains("needs a token or a webhook")
        );
        assert!(
            options(secret(), secret())
                .bot()
                .unwrap_err()
                .contains("not both")
        );
    }

    #[test]
    fn a_bot_can_do_what_a_webhook_cannot() {
        let bot = bot(CHANNEL, false).profile();
        let hook = webhook(false).profile();
        assert!(bot.features.crosspost && bot.features.reads_back && bot.features.interactions);
        assert!(
            !hook.features.crosspost && !hook.features.reads_back && !hook.features.interactions
        );
        assert_eq!(bot.features.pins, Pins::Many);
        assert_eq!(hook.features.pins, Pins::None);
        assert_eq!(bot.limits.title, 256);
        let forum = super::replay::bot(FORUM, true).profile();
        assert!(!forum.features.reads_back);
        assert_eq!(forum.features.pins, Pins::One);
    }
}
