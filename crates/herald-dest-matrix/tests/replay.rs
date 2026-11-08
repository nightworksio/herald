//! Matrix against its fixtures: each step's request as the fixture holds
//! it, and each answer read back into what the ledger records.

#[cfg(test)]
mod replay {
    use herald_core::{
        Announcement, Dialect, Entry, Facts, MentionRoom, MentionStyle, Pins, ReadBack, Sent, Step,
        Text, Threads,
    };
    use herald_dest_matrix::{Matrix, MatrixMarkers, Refusal, RouteOptions, profile};
    use herald_ports::{Markers, MarkersError, Method, Secret};
    use herald_testing::sample::{self, marker, route_marker};
    use herald_testing::{Fixture, Replay};
    use jiff::SignedDuration;

    const HOMESERVER: &str = "https://matrix.example";
    const ROOM: &str = "!abcdefg:matrix.example";

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

    fn matrix() -> Matrix {
        Matrix::new(
            &format!("{HOMESERVER}/"),
            Secret::new("HERALD_MATRIX_TOKEN", "test-token"),
            RouteOptions {
                room: ROOM.to_owned(),
            },
        )
    }

    fn announcement() -> Announcement {
        let mut announcement = sample::announcement();
        announcement.parts = vec![
            Text {
                body: "Added".to_owned(),
                html: Some("<h2>Added</h2>".to_owned()),
            },
            Text {
                body: "- the gate".to_owned(),
                html: Some("<ul>\n<li>the gate</li>\n</ul>".to_owned()),
            },
        ];
        announcement.mentioned = vec!["@ana:matrix.example".to_owned(), "ana".to_owned()];
        announcement.replaces = Some("$old".to_owned());
        announcement
    }

    fn event(id: &str) -> Facts {
        Facts {
            message: Some(id.to_owned()),
            thread: None,
            permalink: Some(format!("https://matrix.to/#/{ROOM}/{id}")),
        }
    }

    fn send(replay: &Replay, step: &Step, entry: &Entry) -> Sent {
        pollster::block_on(matrix().send(replay, step, &announcement(), entry)).unwrap()
    }

    fn posted() -> Entry {
        Entry::new(announcement().key).landed(Step::Post, event("$post"))
    }

    #[test]
    fn a_message_threads_its_overflow_and_is_pinned_in_place_of_the_route_s_earlier_pin() {
        assert_eq!(matrix().api(), HOMESERVER);
        let replay = fixture!("announcement");
        let entry = Entry::new(announcement().key);
        assert_eq!(
            send(&replay, &Step::Post, &entry),
            Sent::Landed(event("$post"))
        );
        let entry = posted();
        assert_eq!(
            send(&replay, &Step::Chunk(1), &entry),
            Sent::Landed(event("$reply"))
        );
        for step in [Step::Pin, Step::Unpin] {
            assert_eq!(
                send(&replay, &step, &entry),
                Sent::Landed(Facts::default()),
                "{step:?}"
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn pins_already_as_a_step_leaves_them_are_not_written_again() {
        let replay = fixture!("pins-unchanged");
        let entry = posted();
        let landed = Sent::Landed(Facts::default());
        assert_eq!(send(&replay, &Step::Pin, &entry), landed);
        assert_eq!(send(&replay, &Step::Pin, &entry), landed);
        let mut alone = announcement();
        alone.replaces = Some("$post".to_owned());
        assert_eq!(
            pollster::block_on(matrix().send(&replay, &Step::Unpin, &alone, &entry)).unwrap(),
            landed
        );
        let refused = Sent::Refused(
            "Matrix refused it with 403: M_FORBIDDEN You don't have permission to post that to the room."
                .to_owned(),
        );
        assert_eq!(send(&replay, &Step::Pin, &entry), refused);
        assert_eq!(send(&replay, &Step::Pin, &entry), refused);
        replay.finish().unwrap();
    }

    #[test]
    fn a_pin_or_an_unpin_is_asked_for_as_a_read_of_the_room_s_pins() {
        for step in [Step::Pin, Step::Unpin] {
            let request = matrix().request(&step, &announcement(), &posted()).unwrap();
            assert_eq!(request.method, Method::Get);
            assert_eq!(
                request.url.expose(),
                "https://matrix.example/_matrix/client/v3/rooms/%21abcdefg%3Amatrix.example/state/m.room.pinned_events"
            );
        }
    }

    #[test]
    fn a_step_needing_an_earlier_one_or_a_thing_matrix_has_not_is_refused_unsent() {
        let replay = Replay::of(Fixture::default());
        let bare = Entry::new(announcement().key);
        assert_eq!(
            send(&replay, &Step::Chunk(1), &bare),
            Sent::Refused(
                "the announcement's message is not recorded, so this step cannot be taken yet"
                    .to_owned()
            )
        );
        assert_eq!(
            send(&replay, &Step::Chunk(2), &posted()),
            Sent::Refused(
                "the message before it is not recorded, so this step cannot be taken yet"
                    .to_owned()
            )
        );
        for (step, said) in [
            (Step::Crosspost, "Matrix cannot crosspost"),
            (
                Step::OpenThread,
                "Matrix cannot open a thread; its replies are the thread",
            ),
            (
                Step::Deliver("https://a.example/inbox".to_owned()),
                "Matrix cannot deliver to an inbox",
            ),
        ] {
            assert_eq!(
                send(&replay, &step, &posted()),
                Sent::Refused(said.to_owned())
            );
        }
        assert_eq!(
            send(&replay, &Step::Pin, &bare),
            Sent::Refused(
                "the announcement's message is not recorded, so this step cannot be taken yet"
                    .to_owned()
            )
        );
        replay.finish().unwrap();
        assert_eq!(
            Refusal::Cannot("crosspost").to_string(),
            "Matrix cannot crosspost"
        );
    }

    fn content(announcement: &Announcement) -> serde_json::Value {
        let request = matrix()
            .request(&Step::Post, announcement, &Entry::new(announcement.key))
            .unwrap();
        serde_json::from_slice(&request.body).unwrap()
    }

    #[test]
    fn mentions_past_the_announcement_s_room_go_in_messages_after_its_parts() {
        let mut many = announcement();
        many.mentioned_after = vec![vec!["@ana:matrix.example".to_owned(), "ana".to_owned()]];
        let after = u32::try_from(many.parts.len()).unwrap();
        let landed = |message: &str| herald_core::Facts {
            message: Some(message.to_owned()),
            ..herald_core::Facts::default()
        };
        let posted = (1..after).fold(
            Entry::new(many.key).landed(Step::Post, landed("$root")),
            |entry, index| entry.landed(Step::Chunk(index), landed(&format!("$chunk{index}"))),
        );
        let request = matrix()
            .request(&Step::Chunk(after), &many, &posted)
            .unwrap();
        let content: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(content["body"], "@ana:matrix.example");
        assert_eq!(
            content["m.mentions"]["user_ids"],
            serde_json::json!(["@ana:matrix.example"])
        );
        assert!(
            content["formatted_body"]
                .as_str()
                .unwrap()
                .contains("<a href=\"https://matrix.to/#/@ana:matrix.example\">")
        );
        let chunk = matrix().request(&Step::Chunk(1), &many, &posted).unwrap();
        let chunk: serde_json::Value = serde_json::from_slice(&chunk.body).unwrap();
        assert_eq!(chunk["m.mentions"], serde_json::json!({}));
    }

    #[test]
    fn only_a_user_written_as_a_matrix_user_id_is_mentioned() {
        let mut mentioning = announcement();
        mentioning.mentioned = [
            "@ana:matrix.example",
            "@bo_b.=-/+9:[::1]:8448",
            "@Ana:matrix.example",
            "@a<b:matrix.example",
            "@ana:matrix<example",
            "@:matrix.example",
            "@ana:",
            "ana",
        ]
        .map(ToOwned::to_owned)
        .to_vec();
        let content = content(&mentioning);
        assert_eq!(
            content["m.mentions"]["user_ids"],
            serde_json::json!(["@ana:matrix.example", "@bo_b.=-/+9:[::1]:8448"])
        );
        assert!(
            content["body"]
                .as_str()
                .unwrap()
                .ends_with("\n\n@ana:matrix.example @bo_b.=-/+9:[::1]:8448")
        );
    }

    #[test]
    fn a_message_mentioning_no_one_carries_no_mention_line_and_its_link_is_made_safe() {
        let mut quiet = announcement();
        quiet.mentioned = Vec::new();
        quiet.link = Some("https://example.org/a\"onclick=\"x".to_owned());
        let content = content(&quiet);
        let body = content["body"].as_str().unwrap();
        let html = content["formatted_body"].as_str().unwrap();
        assert!(!body.ends_with("\n\n"), "{body}");
        assert!(!html.contains("<p></p>"), "{html}");
        assert!(
            html.contains("<a href=\"https://example.org/a%22onclick=%22x"),
            "{html}"
        );
        assert!(!html.contains("\"onclick"), "{html}");
        assert_eq!(content["m.mentions"]["user_ids"], serde_json::json!([]));
    }

    #[test]
    fn a_reply_follows_the_one_before_it_under_the_announcement() {
        let entry = posted().landed(Step::Chunk(1), event("$reply"));
        let request = matrix()
            .request(&Step::Chunk(2), &announcement(), &entry)
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["m.relates_to"]["event_id"], "$post");
        assert_eq!(body["m.relates_to"]["m.in_reply_to"]["event_id"], "$reply");
        assert_eq!(body["body"], "");
        assert!(
            request
                .url
                .redacted()
                .ends_with(&format!("/send/m.room.message/{}.2", announcement().key))
        );
    }

    #[test]
    fn an_unpin_with_no_remembered_pin_unpins_herald_s_own_pins_of_the_route_and_no_others() {
        let replay = fixture!("unpin-by-marker");
        let mut unremembered = announcement();
        unremembered.replaces = None;
        let unpin = || {
            pollster::block_on(matrix().send(&replay, &Step::Unpin, &unremembered, &posted()))
                .unwrap()
        };
        let landed = Sent::Landed(Facts::default());
        assert_eq!(unpin(), landed);
        assert_eq!(unpin(), landed);
        assert_eq!(
            unpin(),
            Sent::Refused("the homeserver did not say which account the token is".to_owned())
        );
        assert_eq!(
            unpin(),
            Sent::Refused(
                "Matrix refused it with 401: M_UNKNOWN_TOKEN Invalid access token".to_owned()
            )
        );
        assert_eq!(unpin(), Sent::RetryAfter(SignedDuration::from_secs(5)));
        replay.finish().unwrap();
    }

    #[test]
    fn a_limit_or_an_outage_is_asked_again_after_the_wait_it_names_or_its_own() {
        let replay = fixture!("rate-limited");
        let entry = Entry::new(announcement().key);
        for wait in [
            SignedDuration::from_millis(2500),
            SignedDuration::from_secs(7),
            SignedDuration::from_secs(5),
            SignedDuration::from_secs(30),
            SignedDuration::from_millis(2500),
            SignedDuration::from_secs(5),
        ] {
            assert_eq!(send(&replay, &Step::Post, &entry), Sent::RetryAfter(wait));
        }
        replay.finish().unwrap();
    }

    #[test]
    fn a_refusal_names_its_status_and_the_homeserver_s_words() {
        let replay = fixture!("refused");
        let entry = Entry::new(announcement().key);
        for said in [
            "Matrix refused it with 403: M_FORBIDDEN User @herald:matrix.example not in room",
            "Matrix refused it with 401",
            "the homeserver's answer holds no event id",
        ] {
            assert_eq!(
                send(&replay, &Step::Post, &entry),
                Sent::Refused(said.to_owned())
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn its_own_marked_message_its_thread_and_its_pin_are_read_back_and_no_one_elses() {
        let matrix = matrix();
        let replay = fixture!("markers");
        let markers = MatrixMarkers::new(&matrix, &replay);
        let key = announcement().key;
        let message = |id: &str| Facts {
            message: Some(id.to_owned()),
            ..Facts::default()
        };
        assert_eq!(
            pollster::block_on(markers.entry(&key, &marker())).unwrap(),
            Some(
                Entry::new(key)
                    .landed(Step::Post, message("$post"))
                    .landed(Step::Chunk(1), message("$reply"))
                    .landed(Step::Chunk(2), message("$reply2"))
                    .landed(Step::Pin, Facts::default())
                    .landed(Step::Unpin, Facts::default())
            )
        );
        assert_eq!(
            pollster::block_on(markers.entry(&key, &marker())).unwrap(),
            None
        );
        assert_eq!(
            pollster::block_on(markers.entry(&key, &marker())).unwrap(),
            Some(Entry::new(key).landed(Step::Post, message("$post")))
        );
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
    fn the_timeline_is_read_back_page_by_page_until_the_post_and_no_further_than_read_back() {
        let matrix = matrix();
        let replay = fixture!("markers-paged");
        let key = announcement().key;
        let message = |id: &str| Facts {
            message: Some(id.to_owned()),
            ..Facts::default()
        };
        assert_eq!(
            pollster::block_on(MatrixMarkers::new(&matrix, &replay).entry(&key, &marker()))
                .unwrap(),
            Some(Entry::new(key).landed(Step::Post, message("$post")))
        );
        let one_page = matrix.reading_back(ReadBack::new(1).unwrap());
        assert_eq!(
            pollster::block_on(MatrixMarkers::new(&one_page, &replay).entry(&key, &marker()))
                .unwrap(),
            None
        );
        replay.finish().unwrap();
    }

    #[test]
    fn a_room_that_cannot_be_read_says_why() {
        let matrix = matrix();
        let replay = fixture!("markers-unreadable");
        let markers = MatrixMarkers::new(&matrix, &replay);
        let key = announcement().key;
        for reason in [
            "the homeserver answered 401",
            "the homeserver did not say which account the token is",
            "the homeserver answered 500 for the room's pins",
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
    fn a_room_is_named_by_its_id_and_matrix_does_what_its_profile_says() {
        assert_eq!(
            RouteOptions {
                room: ROOM.to_owned()
            }
            .check(),
            Ok(())
        );
        for room in [
            "#herald:matrix.example",
            "!abcdefg",
            "!:matrix.example",
            "!abc:",
        ] {
            assert_eq!(
                RouteOptions {
                    room: room.to_owned()
                }
                .check(),
                Err(format!(
                    "\"{room}\" is not a room id; give the room's id, such as \"!abcdefg:matrix.org\", from its settings"
                )),
                "{room}"
            );
        }
        let profile = profile();
        assert_eq!(matrix().profile(), profile);
        assert_eq!(profile.dialect, Dialect::Matrix);
        assert_eq!(profile.features.threads, Threads::Replies);
        assert_eq!(profile.features.pins, Pins::Many);
        assert_eq!(
            profile.features.mentions,
            MentionStyle::Users(MentionRoom {
                first: 24_576,
                rest: 57_344,
                copies: 4,
                overhead: 64,
            })
        );
        assert!(profile.features.reads_back);
        assert!(profile.features.edits);
        assert!(!profile.features.crosspost);
        assert!(!profile.features.interactions);
        assert_eq!(
            (
                profile.limits.title,
                profile.limits.text,
                profile.limits.followup
            ),
            (256, 16_384, 16_384)
        );
    }
}
