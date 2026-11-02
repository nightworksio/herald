//! Mastodon against its fixtures: each step's request as the fixture holds
//! it, and each answer read back into what the ledger records.

#[cfg(test)]
mod replay {
    use herald_core::{Dialect, Entry, Facts, MentionStyle, Pins, Profile, Sent, Step, Threads};
    use herald_dest_fediverse::{
        LIMIT, Mastodon, MastodonMarkers, Refusal, RouteOptions, Visibility, profile, shortest,
    };
    use herald_ports::{Markers, MarkersError, Secret};
    use herald_testing::sample::{announcement, marker, route_marker};
    use herald_testing::{Fixture, Replay};
    use jiff::SignedDuration;

    const SERVER: &str = "https://mastodon.example";
    const POST: &str = "113420000000000001";
    const REPLY: &str = "113420000000000002";

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

    fn mastodon(route: RouteOptions) -> Mastodon {
        Mastodon::new(
            &format!("{SERVER}/"),
            Secret::new("HERALD_MASTODON_TOKEN", "test-token"),
            route,
        )
    }

    fn status(id: &str) -> Facts {
        Facts {
            message: Some(id.to_owned()),
            thread: None,
            permalink: Some(format!("{SERVER}/@herald/{id}")),
        }
    }

    fn send(mastodon: &Mastodon, replay: &Replay, step: &Step, entry: &Entry) -> Sent {
        pollster::block_on(mastodon.send(replay, step, &announcement(), entry)).unwrap()
    }

    fn posted() -> Entry {
        Entry::new(announcement().key).landed(Step::Post, status(POST))
    }

    #[test]
    fn a_status_carries_the_title_text_and_marked_link_and_the_overflow_replies_under_it() {
        let mastodon = mastodon(RouteOptions::default());
        assert_eq!(mastodon.api(), SERVER);
        let replay = fixture!("announcement");
        let entry = Entry::new(announcement().key);
        assert_eq!(
            send(&mastodon, &replay, &Step::Post, &entry),
            Sent::Landed(status(POST))
        );
        assert_eq!(
            send(&mastodon, &replay, &Step::Chunk(1), &posted()),
            Sent::Landed(status(REPLY))
        );
        replay.finish().unwrap();
    }

    #[test]
    fn a_route_gives_each_status_its_visibility_language_and_content_warning() {
        let mastodon = mastodon(RouteOptions {
            visibility: Visibility::Unlisted,
            language: Some("en".to_owned()),
            content_warning: Some("Release notes".to_owned()),
            limit: None,
        });
        let replay = fixture!("options");
        assert_eq!(
            send(
                &mastodon,
                &replay,
                &Step::Post,
                &Entry::new(announcement().key)
            ),
            Sent::Landed(status("113420000000000003"))
        );
        replay.finish().unwrap();
    }

    #[test]
    fn a_reply_follows_the_status_before_it_and_waits_for_it() {
        let mastodon = mastodon(RouteOptions::default());
        let request = mastodon
            .request(
                &Step::Chunk(2),
                &announcement(),
                &posted().landed(Step::Chunk(1), status(REPLY)),
            )
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["in_reply_to_id"], REPLY);
        assert_eq!(body["status"], "");
        let key = request
            .headers
            .iter()
            .find(|(name, _)| name == "Idempotency-Key")
            .map(|(_, value)| value.redacted());
        assert_eq!(key, Some(format!("{}.2", announcement().key)));
        assert_eq!(
            mastodon.request(&Step::Chunk(2), &announcement(), &posted()),
            Err(Refusal::Missing("the status before it"))
        );
        assert_eq!(
            mastodon.request(
                &Step::Chunk(1),
                &announcement(),
                &Entry::new(announcement().key)
            ),
            Err(Refusal::Missing("the announcement's status"))
        );
        assert_eq!(
            Refusal::Missing("the status before it").to_string(),
            "the status before it is not recorded, so this step cannot be taken yet"
        );
    }

    #[test]
    fn a_status_with_no_text_holds_its_title_and_link_alone() {
        let mut bare = announcement();
        bare.parts.clear();
        let request = mastodon(RouteOptions::default())
            .request(&Step::Post, &bare, &Entry::new(bare.key))
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let link = bare.marks.on(bare.link.as_deref().unwrap());
        assert_eq!(body["status"], format!("{}\n\n{link}", bare.title));
        let mut blank = announcement();
        blank.parts.truncate(1);
        blank.parts[0].body.clear();
        let request = mastodon(RouteOptions::default())
            .request(&Step::Post, &blank, &Entry::new(blank.key))
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["status"], format!("{}\n\n{link}", blank.title));
    }

    #[test]
    fn what_mastodon_has_no_way_to_do_is_refused_and_sends_nothing() {
        let mastodon = mastodon(RouteOptions::default());
        let replay = Replay::of(Fixture::default());
        let entry = posted();
        for (step, reason) in [
            (Step::Crosspost, "Mastodon cannot crosspost"),
            (
                Step::OpenThread,
                "Mastodon cannot open a thread; its replies are the thread",
            ),
            (Step::Pin, "Mastodon cannot pin a route's announcement"),
            (Step::Unpin, "Mastodon cannot pin a route's announcement"),
            (
                Step::Deliver("https://a.example/inbox".to_owned()),
                "Mastodon cannot deliver to an inbox",
            ),
        ] {
            assert_eq!(
                send(&mastodon, &replay, &step, &entry),
                Sent::Refused(reason.to_owned()),
                "{step:?}"
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn a_limit_or_an_outage_is_asked_again_after_the_wait_it_names_or_its_own() {
        let mastodon = mastodon(RouteOptions::default());
        let replay = fixture!("rate-limited");
        let entry = Entry::new(announcement().key);
        for wait in [
            SignedDuration::from_mins(5),
            SignedDuration::from_secs(30),
            SignedDuration::from_secs(5),
            SignedDuration::from_mins(1),
        ] {
            assert_eq!(
                send(&mastodon, &replay, &Step::Post, &entry),
                Sent::RetryAfter(wait)
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn a_refusal_names_the_status_and_what_mastodon_said() {
        let mastodon = mastodon(RouteOptions::default());
        let replay = fixture!("refused");
        let entry = Entry::new(announcement().key);
        for said in [
            "Mastodon refused it with 422: Validation failed: Text character limit of 500 exceeded",
            "Mastodon refused it with 401",
        ] {
            assert_eq!(
                send(&mastodon, &replay, &Step::Post, &entry),
                Sent::Refused(said.to_owned())
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn an_answer_without_an_id_is_not_taken_for_a_landed_status() {
        let mastodon = mastodon(RouteOptions::default());
        let replay = fixture!("answer-without-id");
        assert_eq!(
            send(
                &mastodon,
                &replay,
                &Step::Post,
                &Entry::new(announcement().key)
            ),
            Sent::Refused("Mastodon's answer holds no id".to_owned())
        );
        replay.finish().unwrap();
    }

    #[test]
    fn its_own_marked_status_and_reply_chain_are_read_back_and_no_one_elses() {
        let mastodon = mastodon(RouteOptions::default());
        let replay = fixture!("markers");
        let markers = MastodonMarkers::new(&mastodon, &replay);
        let key = announcement().key;
        let read = pollster::block_on(markers.entry(&key, &marker())).unwrap();
        assert_eq!(read, Some(posted().landed(Step::Chunk(1), status(REPLY))));
        assert_eq!(
            pollster::block_on(markers.entry(&key, &marker())).unwrap(),
            None
        );
        assert_eq!(
            pollster::block_on(markers.threads(&route_marker())).unwrap(),
            []
        );
        replay.finish().unwrap();
    }

    #[test]
    fn an_account_that_cannot_be_read_says_why() {
        let mastodon = mastodon(RouteOptions::default());
        let replay = fixture!("markers-unreadable");
        let markers = MastodonMarkers::new(&mastodon, &replay);
        let key = announcement().key;
        for reason in [
            "Mastodon answered 401",
            "Mastodon did not say which account the token is",
        ] {
            assert_eq!(
                pollster::block_on(markers.entry(&key, &marker())),
                Err(MarkersError::Unreadable {
                    reason: reason.to_owned()
                })
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn a_status_fits_the_server_limit_with_its_title_link_and_warning() {
        let plain = profile(&RouteOptions::default());
        assert_eq!(plain.dialect, Dialect::Plain);
        assert_eq!(plain.features.threads, Threads::Replies);
        assert_eq!(plain.features.pins, Pins::None);
        assert_eq!(plain.features.mentions, MentionStyle::None);
        assert!(plain.features.reads_back);
        assert!(plain.features.edits);
        assert!(!plain.features.crosspost);
        assert_eq!(plain.limits.title, 100);
        assert_eq!(plain.limits.text, LIMIT - 127);
        assert_eq!(plain.limits.followup, LIMIT);
        let warned = RouteOptions {
            content_warning: Some("CW".to_owned()),
            limit: Some(1000),
            ..RouteOptions::default()
        };
        let Profile { limits, .. } = profile(&warned);
        assert_eq!((limits.text, limits.followup), (1000 - 129, 998));
        assert_eq!(shortest(&warned), 130);
        assert_eq!(shortest(&RouteOptions::default()), 128);
        let accented = RouteOptions {
            content_warning: Some("Änderungen".to_owned()),
            ..RouteOptions::default()
        };
        assert_eq!(shortest(&accented), 138);
        let tight = RouteOptions {
            limit: Some(10),
            content_warning: Some("a long warning".to_owned()),
            ..RouteOptions::default()
        };
        assert_eq!(
            (profile(&tight).limits.text, profile(&tight).limits.followup),
            (1, 1)
        );
    }

    #[test]
    fn a_route_checks_its_language_and_limit() {
        assert_eq!(RouteOptions::default().check(), Ok(()));
        for language in ["en", "deu"] {
            let options = RouteOptions {
                language: Some(language.to_owned()),
                limit: Some(128),
                ..RouteOptions::default()
            };
            assert_eq!(options.check(), Ok(()), "{language}");
        }
        for language in ["e", "engl", "EN", "e1"] {
            let options = RouteOptions {
                language: Some(language.to_owned()),
                ..RouteOptions::default()
            };
            assert_eq!(
                options.check(),
                Err(format!(
                    "\"{language}\" is not an ISO 639 language code; give two or three lowercase letters, such as \"en\""
                )),
                "{language}"
            );
        }
        let short = RouteOptions {
            limit: Some(127),
            ..RouteOptions::default()
        };
        assert_eq!(
            short.check(),
            Err("a status needs at least 128 characters for its title, link and content warning; raise limit to the server's own".to_owned())
        );
        assert_eq!(Visibility::Private.as_str(), "private");
        assert_eq!(Visibility::Public.as_str(), "public");
    }
}
