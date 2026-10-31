//! Discord against its fixtures: each step's request as the fixture holds
//! it, and each answer read back into what the ledger records.

#[cfg(test)]
mod replay {
    use herald_core::{
        Announcement, Chore, Delivery, Entry, Facts, Identity, Key, Mentions, RoleId, RouteId,
        Sent, Step, Text, Thread,
    };
    use herald_dest_discord::{Credential, Discord, DiscordMarkers, RouteOptions};
    use herald_ports::{Markers, Secret};
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

    fn announcement() -> Announcement {
        Announcement {
            key: Key::derive(
                &RouteId::new("php-mutation-gate-releases"),
                &Identity::new("github.com/1066469473/v0.1.0"),
            ),
            route: RouteId::new("php-mutation-gate-releases"),
            title: "📦 php-mutation-gate v0.1.0".to_owned(),
            parts: vec![
                Text {
                    body: "## Added".to_owned(),
                    html: None,
                },
                Text {
                    body: "- the gate".to_owned(),
                    html: None,
                },
            ],
            thread: Some("php-mutation-gate v0.1.0".to_owned()),
            link: Some(
                "https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0".to_owned(),
            ),
            prerelease: false,
            occurred: "2026-10-31T12:00:00Z".parse().unwrap(),
        }
    }

    fn delivery() -> Delivery {
        Delivery {
            crosspost: true,
            thread: Some(Thread {
                name: String::new(),
                archive_after: None,
                lock_after: None,
            }),
            pin: true,
            mentions: Mentions {
                roles: vec![RoleId::new("1566190000000000001")],
                subscribers: false,
            },
        }
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
        replay.finish().unwrap();
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
        for step in [Step::Crosspost, Step::OpenThread, Step::Pin] {
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
    fn housekeeping_archives_and_locks_threads() {
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

    #[test]
    fn markers_read_back_which_steps_landed() {
        let replay = fixture!("markers");
        let discord = bot(CHANNEL, false);
        let entry =
            pollster::block_on(DiscordMarkers::new(&discord, &replay).entry(&announcement().key))
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
        let replay = fixture!("markers-unthreaded");
        let discord = bot(CHANNEL, false);
        let entry =
            pollster::block_on(DiscordMarkers::new(&discord, &replay).entry(&announcement().key))
                .unwrap();
        assert_eq!(
            entry,
            Some(Entry::new(announcement().key).landed(Step::Post, message("1566200000000000001")))
        );
        let other = Key::derive(&RouteId::new("other"), &Identity::new("other"));
        let replay = fixture!("markers-unthreaded");
        assert_eq!(
            pollster::block_on(DiscordMarkers::new(&discord, &replay).entry(&other)).unwrap(),
            None
        );
    }

    #[test]
    fn markers_that_cannot_be_read_say_why() {
        let discord = bot(CHANNEL, false);
        let replay = fixture!("markers-unreadable");
        let error =
            pollster::block_on(DiscordMarkers::new(&discord, &replay).entry(&announcement().key))
                .unwrap_err();
        assert_eq!(
            error.to_string(),
            "the destination's posts could not be read: Discord answered 403"
        );
        let nothing = Replay::of(Fixture::default());
        assert!(
            pollster::block_on(DiscordMarkers::new(&discord, &nothing).entry(&announcement().key))
                .is_err()
        );
        let hook = webhook(false);
        let error =
            pollster::block_on(DiscordMarkers::new(&hook, &nothing).entry(&announcement().key))
                .unwrap_err();
        assert_eq!(
            error.to_string(),
            "the destination's posts could not be read: a webhook cannot read a channel's messages"
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
    fn a_bot_can_do_what_a_webhook_cannot() {
        let bot = bot(CHANNEL, false).profile();
        let hook = webhook(false).profile();
        assert!(
            bot.features.crosspost
                && bot.features.pins
                && bot.features.reads_back
                && bot.features.interactions
        );
        assert!(
            !hook.features.crosspost
                && !hook.features.pins
                && !hook.features.reads_back
                && !hook.features.interactions
        );
        assert_eq!(bot.limits.title, 256);
        assert!(
            !super::replay::bot(FORUM, true)
                .profile()
                .features
                .reads_back
        );
    }
}
