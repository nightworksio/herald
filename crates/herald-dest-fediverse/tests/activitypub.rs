//! herald's own ActivityPub actor: its documents, each delivery as the
//! fixture holds it, and each inbox's answer read into what the ledger
//! records.

#[cfg(test)]
mod activitypub {
    use herald_core::{Entry, Facts, MentionStyle, Pins, Sent, Step, Text, Threads, Waits};
    use herald_dest_fediverse::{
        ActivityPub, ActorOptions, ActorRouteOptions, Approve, FollowerCaps, InboxActivity, Mode,
        accept, actor_document, actor_profile, asked, create, note, webfinger,
    };
    use herald_ports::{PUBLIC_BASE, Secret};
    use herald_testing::sample;
    use herald_testing::{FakeSigner, Fixture, Replay};
    use jiff::{SignedDuration, Timestamp};
    use serde_json::json;

    const DOMAIN: &str = "herald.example";
    const ACTOR: &str = "https://herald.example/users/php-mutation-gate";

    fn plain_route() -> ActorRouteOptions {
        ActorRouteOptions {
            actor: "php-mutation-gate".to_owned(),
            content_warning: None,
            visibility: herald_dest_fediverse::Audience::Public,
            language: None,
        }
    }

    fn reached(key: &str) -> ActivityPub {
        ActivityPub::new(
            DOMAIN,
            "",
            Secret::new(key, "the-key"),
            ActorRouteOptions {
                actor: "php-mutation-gate".to_owned(),
                content_warning: Some("Release notes".to_owned()),
                visibility: herald_dest_fediverse::Audience::Public,
                language: None,
            },
        )
    }

    #[test]
    fn an_actor_served_under_a_base_names_every_id_under_it_and_keeps_its_handle() {
        let based = ActivityPub::new(
            DOMAIN,
            "/herald",
            Secret::new("HERALD_ACTIVITYPUB_KEY", "the-key"),
            ActorRouteOptions {
                actor: "php-mutation-gate".to_owned(),
                content_warning: None,
                visibility: herald_dest_fediverse::Audience::Public,
                language: None,
            },
        );
        assert_eq!(
            based.id(),
            "https://herald.example/herald/users/php-mutation-gate"
        );
        assert_eq!(based.domain(), DOMAIN);
        let finger: serde_json::Value = serde_json::from_str(&based.finger()).unwrap();
        assert_eq!(finger["subject"], "acct:php-mutation-gate@herald.example");
        assert_eq!(
            finger["links"][0]["href"],
            "https://herald.example/herald/users/php-mutation-gate"
        );
        let document = actor_document(
            "herald.example/herald",
            "php-mutation-gate",
            &[1; 70],
            false,
        );
        assert_eq!(
            document["endpoints"]["sharedInbox"],
            "https://herald.example/herald/inbox"
        );
    }

    fn announcement() -> herald_core::Announcement {
        let mut announcement = sample::announcement();
        announcement.parts = vec![Text {
            body: "Added".to_owned(),
            html: Some("<h2>Added</h2>\n<ul>\n<li>the gate</li>\n</ul>".to_owned()),
        }];
        announcement
    }

    fn now() -> Timestamp {
        "2026-11-03T08:00:00Z".parse().unwrap()
    }

    fn send(actor: &ActivityPub, replay: &Replay, step: &Step) -> Sent {
        let announcement = announcement();
        pollster::block_on(actor.send(
            replay,
            &FakeSigner,
            now(),
            step,
            &announcement,
            &Entry::new(announcement.key),
        ))
        .unwrap()
    }

    fn deliver(inbox: &str) -> Step {
        Step::Deliver(inbox.to_owned())
    }

    #[test]
    fn each_inbox_takes_the_signed_create_of_the_note_and_its_answer_is_read() {
        let replay = Replay::of(
            Fixture::from_json(include_str!("../fixtures/deliveries.reference.json")).unwrap(),
        );
        let actor = reached("HERALD_ACTIVITYPUB_KEY");
        let landed = Sent::Landed(Facts::default());
        for (inbox, sent) in [
            ("https://social.example/inbox", landed.clone()),
            ("https://gone.example/inbox", landed.clone()),
            (
                "https://busy.example/inbox",
                Sent::RetryAfter(SignedDuration::from_secs(9)),
            ),
            (
                "https://down.example/inbox",
                Sent::RetryAfter(SignedDuration::from_secs(5)),
            ),
            (
                "https://strict.example/users/ana/inbox",
                Sent::Refused("the inbox refused it with 401; check the credential herald sends, and that it has not expired".to_owned()),
            ),
            ("https://odd.example:8443/inbox?shared=1", landed),
        ] {
            assert_eq!(send(&actor, &replay, &deliver(inbox)), sent, "{inbox}");
        }
        replay.finish().unwrap();
    }

    #[test]
    fn an_inbox_that_names_no_wait_is_asked_again_as_long_as_the_actor_says() {
        let replay = Replay::of(
            Fixture::from_json(include_str!("../fixtures/deliveries.reference.json")).unwrap(),
        );
        let actor = reached("HERALD_ACTIVITYPUB_KEY").waiting(Waits {
            outage: SignedDuration::from_secs(40),
            limited: SignedDuration::from_secs(90),
        });
        let mut waited = Vec::new();
        for inbox in [
            "https://social.example/inbox",
            "https://gone.example/inbox",
            "https://busy.example/inbox",
            "https://down.example/inbox",
        ] {
            if let Sent::RetryAfter(wait) = send(&actor, &replay, &deliver(inbox)) {
                waited.push(wait);
            }
        }
        assert_eq!(
            waited,
            [SignedDuration::from_secs(9), SignedDuration::from_secs(40)]
        );
    }

    #[test]
    fn the_post_is_the_note_itself_and_nothing_else_is_asked() {
        let replay = Replay::of(Fixture::default());
        let actor = reached("HERALD_ACTIVITYPUB_KEY");
        let note = format!("{ACTOR}/notes/{}", announcement().key);
        assert_eq!(
            send(&actor, &replay, &Step::Post),
            Sent::Landed(Facts {
                message: Some(note.clone()),
                thread: None,
                permalink: Some(note),
            })
        );
        assert_eq!(
            send(&actor, &replay, &Step::Chunk(1)),
            Sent::Landed(Facts::default())
        );
        for (step, what) in [
            (Step::Crosspost, "crosspost"),
            (Step::OpenThread, "open a thread"),
            (Step::Pin, "pin"),
            (Step::Unpin, "pin"),
            (Step::Deliveries, "deliver to every inbox in one request"),
        ] {
            assert_eq!(
                send(&actor, &replay, &step),
                Sent::Refused(format!("herald's ActivityPub actor cannot {what}"))
            );
        }
        for inbox in [
            "http://social.example/inbox",
            "not a url",
            "ftp://social.example/inbox",
        ] {
            assert_eq!(
                send(&actor, &replay, &deliver(inbox)),
                Sent::Refused(format!(
                    "\"{inbox}\" is not an https inbox, so nothing was delivered to it"
                ))
            );
        }
        assert_eq!(
            send(
                &reached("BAD"),
                &replay,
                &deliver("https://social.example/inbox")
            ),
            Sent::Refused(
                "the secret BAD is not a key for this signature; give a PKCS#8 private key as PEM"
                    .to_owned()
            )
        );
        let quoted = ActivityPub::new(
            DOMAIN,
            "",
            Secret::new("HERALD_ACTIVITYPUB_KEY", "the-key"),
            ActorRouteOptions {
                actor: "a\"b".to_owned(),
                ..plain_route()
            },
        );
        assert_eq!(
            send(&quoted, &replay, &deliver("https://social.example/inbox")),
            Sent::Refused(
                "the key id https://herald.example/users/a\"b#main-key holds a quote, which no Signature header can carry; name the actor without one"
                    .to_owned()
            )
        );
        replay.finish().unwrap();
        assert_eq!(actor.api(), PUBLIC_BASE);
        assert_eq!(actor.actor(), "php-mutation-gate");
        assert_eq!(actor.id(), ACTOR);
        assert_eq!(actor.profile(), actor_profile());
    }

    #[test]
    fn its_documents_name_the_actor_its_key_and_its_inboxes() {
        let document = actor_document(DOMAIN, "php-mutation-gate", &[1; 70], true);
        assert_eq!(document["id"], ACTOR);
        assert_eq!(document["type"], "Service");
        assert_eq!(document["preferredUsername"], "php-mutation-gate");
        assert_eq!(document["inbox"], format!("{ACTOR}/inbox"));
        assert_eq!(document["followers"], format!("{ACTOR}/followers"));
        assert_eq!(document["outbox"], format!("{ACTOR}/outbox"));
        assert_eq!(document["manuallyApprovesFollowers"], true);
        assert_eq!(
            document["endpoints"]["sharedInbox"],
            "https://herald.example/inbox"
        );
        assert_eq!(document["publicKey"]["id"], format!("{ACTOR}#main-key"));
        assert_eq!(document["publicKey"]["owner"], ACTOR);
        let pem = document["publicKey"]["publicKeyPem"].as_str().unwrap();
        let lines: Vec<&str> = pem.lines().collect();
        assert_eq!(lines[0], "-----BEGIN PUBLIC KEY-----");
        assert_eq!(lines[1].len(), 64);
        assert_eq!(lines[2].len(), 32);
        assert_eq!(lines[3], "-----END PUBLIC KEY-----");
        assert!(pem.ends_with('\n'));
        assert_eq!(
            webfinger(DOMAIN, DOMAIN, "php-mutation-gate"),
            json!({
                "subject": "acct:php-mutation-gate@herald.example",
                "aliases": [ACTOR],
                "links": [{ "rel": "self", "type": "application/activity+json", "href": ACTOR }],
            })
        );
        let mut bare = announcement();
        bare.link = None;
        bare.title = herald_core::Title::new("a <b> & c");
        bare.parts.push(Text {
            body: String::new(),
            html: Some(String::new()),
        });
        let plain = note(DOMAIN, &bare, &plain_route());
        assert_eq!(
            plain["content"],
            "<p><strong>a &lt;b&gt; &amp; c</strong></p>\n<h2>Added</h2>\n<ul>\n<li>the gate</li>\n</ul>"
        );
        assert_eq!(plain["sensitive"], false);
        assert!(plain.get("summary").is_none() && plain.get("url").is_none());
        let created = create(DOMAIN, &bare, &plain_route());
        assert_eq!(
            created["id"],
            format!("{}/activity", plain["id"].as_str().unwrap())
        );
        assert_eq!(created["object"], plain);
        assert_eq!(created["actor"], ACTOR);
    }

    #[test]
    fn a_domain_and_an_actor_are_written_as_followers_write_them() {
        let options = |domain: &str| ActorOptions {
            domain: domain.to_owned(),
            symbols: None,
            waits: None,
            outbox_page: None,
            deliveries: None,
            mode: Mode::Open,
            block: Vec::new(),
            allow: Vec::new(),
            followers: None,
            approve: Approve::Auto,
            lists: Vec::new(),
            key: herald_core::SecretRef {
                secret: "K".to_owned(),
            },
        };
        for domain in ["herald.example", "a-b.c1.example"] {
            assert_eq!(options(domain).check(), Ok(()), "{domain}");
        }
        for domain in [
            "https://herald.example",
            "herald.example:443",
            "herald.example/users",
            "herald",
            "Herald.example",
            "-herald.example",
            "herald-.example",
            "herald..example",
        ] {
            assert_eq!(
                options(domain).check(),
                Err(format!(
                    "\"{domain}\" is not a domain; give the host herald serves its actors on, such as \"herald.example\", with no scheme, port or path"
                )),
                "{domain}"
            );
        }
        let route = |actor: &str| ActorRouteOptions {
            actor: actor.to_owned(),
            content_warning: None,
            visibility: herald_dest_fediverse::Audience::Public,
            language: None,
        };
        for actor in ["php-mutation-gate", "herald", "a_b", "x", &"a".repeat(30)] {
            assert_eq!(route(actor).check(), Ok(()), "{actor}");
        }
        for actor in ["", "PHP", "-gate", "gate-", "a.b", "a b", &"a".repeat(31)] {
            assert_eq!(
                route(actor).check(),
                Err(format!(
                    "\"{actor}\" is not an actor's name; give at most 30 lowercase letters, digits, `_` or a `-` within, such as \"php-mutation-gate\""
                )),
                "{actor:?}"
            );
        }
        let profile = actor_profile();
        assert_eq!(profile.markup, herald_dest_fediverse::ACTOR_MARKUP);
        assert_eq!(profile.features.threads, Threads::None);
        assert_eq!(profile.features.pins, Pins::None);
        assert_eq!(profile.features.mentions, MentionStyle::None);
        assert!(!profile.features.reads_back && !profile.features.edits);
        assert!(!profile.features.crosspost && !profile.features.interactions);
        assert_eq!((profile.limits.title, profile.limits.text), (500, 20_000));
    }

    const ANA: &str = "https://social.example/users/ana";

    fn asking(activity: &serde_json::Value) -> InboxActivity {
        asked(activity.to_string().as_bytes(), ANA)
    }

    fn nothing(why: &str) -> InboxActivity {
        InboxActivity::Nothing(why.to_owned())
    }

    #[test]
    fn an_outbox_page_holds_one_to_eighty_notes_and_twenty_where_unset() {
        let paged = |outbox_page| ActorOptions {
            domain: "herald.example".to_owned(),
            symbols: None,
            waits: None,
            outbox_page,
            deliveries: None,
            mode: Mode::Open,
            block: Vec::new(),
            allow: Vec::new(),
            followers: None,
            approve: Approve::Auto,
            lists: Vec::new(),
            key: herald_core::SecretRef {
                secret: "K".to_owned(),
            },
        };
        assert_eq!(paged(None).outbox_page(), 20);
        assert_eq!(paged(None).deliveries(), 8);
        let wide = ActorOptions {
            deliveries: std::num::NonZeroUsize::new(16),
            ..paged(None)
        };
        assert_eq!(wide.deliveries(), 16);
        for most in [1, 80] {
            assert_eq!(paged(Some(most)).check(), Ok(()), "{most}");
            assert_eq!(paged(Some(most)).outbox_page(), most);
        }
        for most in [0, 81] {
            assert_eq!(
                paged(Some(most)).check(),
                Err(format!("outbox_page holds 1 to 80 Notes, not {most}"))
            );
        }
    }

    #[test]
    fn an_actor_s_lists_name_domains_or_actors_and_its_caps_are_at_least_one() {
        let governed = |block: &[&str], followers: Option<FollowerCaps>| ActorOptions {
            domain: "herald.example".to_owned(),
            symbols: None,
            waits: None,
            outbox_page: None,
            deliveries: None,
            mode: Mode::Allowlist,
            block: block.iter().map(|entry| (*entry).to_owned()).collect(),
            allow: vec!["friends.example".to_owned()],
            followers,
            approve: Approve::Manual,
            lists: Vec::new(),
            key: herald_core::SecretRef {
                secret: "K".to_owned(),
            },
        };
        let open = governed(&["spam.example", "https://good.example/users/troll"], None);
        assert_eq!(open.check(), Ok(()));
        assert!(open.policy().admits("https://friends.example/users/ana"));
        assert!(!open.policy().admits("https://spam.example/users/x"));
        assert!(!open.policy().admits("https://elsewhere.example/users/x"));
        let refused = governed(&["http://spam.example"], None)
            .check()
            .unwrap_err();
        assert!(
            refused.starts_with("\"http://spam.example\" is neither a domain nor an actor's id"),
            "{refused}"
        );
        for caps in [
            FollowerCaps {
                most: Some(0),
                per_domain: None,
            },
            FollowerCaps {
                most: None,
                per_domain: Some(0),
            },
        ] {
            assert_eq!(
                governed(&[], Some(caps)).check(),
                Err("followers.most and followers.per_domain take at least 1; leave one out for no limit".to_owned())
            );
        }
        let capped = governed(
            &[],
            Some(FollowerCaps {
                most: Some(1),
                per_domain: None,
            }),
        );
        assert_eq!(capped.check(), Ok(()));
        assert!(!capped.policy().room(
            "https://friends.example/users/b",
            &["https://friends.example/users/a"]
        ));
    }

    #[test]
    fn a_follow_names_the_actor_it_follows_and_is_carried_whole() {
        let follow = json!({ "id": "https://social.example/f/1", "type": "Follow", "actor": ANA, "object": ACTOR });
        assert_eq!(
            asking(&follow),
            InboxActivity::Follow {
                target: ACTOR.to_owned(),
                follow: follow.clone(),
            }
        );
        let embedded = json!({ "id": "https://social.example/f/2", "type": "Follow", "actor": ANA, "object": { "id": ACTOR } });
        assert_eq!(
            asking(&embedded),
            InboxActivity::Follow {
                target: ACTOR.to_owned(),
                follow: embedded.clone(),
            }
        );
        let unnamed = json!({ "type": "Follow", "actor": ANA, "object": ACTOR });
        assert_eq!(asking(&unnamed), nothing("it is a follow with no id"));
        let aimless = json!({ "id": "https://social.example/f/3", "type": "Follow", "actor": ANA });
        assert_eq!(asking(&aimless), nothing("it follows no one"));
    }

    #[test]
    fn an_undo_takes_back_only_its_sender_s_own_follow() {
        let undo = |actor: &str, object: serde_json::Value| json!({ "id": "https://social.example/u/1", "type": "Undo", "actor": ANA, "object": { "type": "Follow", "actor": actor, "object": object } });
        assert_eq!(
            asking(&undo(ANA, json!(ACTOR))),
            InboxActivity::Undo {
                follower: ANA.to_owned(),
                target: ACTOR.to_owned(),
            }
        );
        assert_eq!(
            asking(&undo("https://social.example/users/bo", json!(ACTOR))),
            nothing("it undoes another actor's follow")
        );
        assert_eq!(
            asking(&undo(ANA, json!(null))),
            nothing("it undoes a follow of no one")
        );
        let liked = json!({ "type": "Undo", "actor": ANA, "object": { "type": "Like", "actor": ANA, "object": ACTOR } });
        assert_eq!(asking(&liked), nothing("it undoes no follow it carries"));
        let referenced =
            json!({ "type": "Undo", "actor": ANA, "object": "https://social.example/f/1" });
        assert_eq!(
            asking(&referenced),
            nothing("it undoes no follow it carries")
        );
    }

    #[test]
    fn anything_but_a_follow_or_its_undoing_is_left_alone() {
        assert_eq!(
            asking(&json!({ "type": "Like", "actor": ANA, "object": ACTOR })),
            nothing("herald takes no Like")
        );
        assert_eq!(
            asking(&json!({ "actor": ANA })),
            nothing("it names no type")
        );
        assert_eq!(asked(b"{", ANA), nothing("it is not JSON"));
    }

    #[test]
    fn an_accept_answers_the_follow_under_an_id_its_follow_alone_decides() {
        let follow = json!({ "id": "https://social.example/f/1", "type": "Follow", "actor": ANA, "object": ACTOR });
        let accepted = accept(DOMAIN, "php-mutation-gate", &follow);
        assert_eq!(accepted["type"], "Accept");
        assert_eq!(accepted["actor"], ACTOR);
        assert_eq!(accepted["object"], follow);
        assert_eq!(
            accepted["@context"],
            "https://www.w3.org/ns/activitystreams"
        );
        let id = accepted["id"].as_str().unwrap();
        assert!(id.starts_with(&format!("{ACTOR}/accepts/")), "{id}");
        assert_eq!(accept(DOMAIN, "php-mutation-gate", &follow)["id"], id);
        let other = json!({ "id": "https://social.example/f/2", "type": "Follow", "actor": ANA, "object": ACTOR });
        assert_ne!(accept(DOMAIN, "php-mutation-gate", &other)["id"], id);
        assert_eq!(reached("HERALD_ACTIVITYPUB_KEY").accept(&follow), accepted);
    }

    #[test]
    fn the_actor_serves_its_own_document_webfinger_and_notes() {
        let actor = reached("HERALD_ACTIVITYPUB_KEY");
        assert_eq!(actor.domain(), DOMAIN);
        let document: serde_json::Value =
            serde_json::from_str(&pollster::block_on(actor.document(&FakeSigner)).unwrap())
                .unwrap();
        assert_eq!(
            document,
            actor_document(
                DOMAIN,
                "php-mutation-gate",
                &pollster::block_on(herald_ports::Signer::public(
                    &FakeSigner,
                    herald_ports::Algorithm::RsaSha256,
                    &Secret::new("HERALD_ACTIVITYPUB_KEY", "the-key"),
                ))
                .unwrap(),
                false,
            )
        );
        let manual = reached("HERALD_ACTIVITYPUB_KEY")
            .governed(herald_dest_fediverse::Policy::default().approving(Approve::Manual));
        let waiting: serde_json::Value =
            serde_json::from_str(&pollster::block_on(manual.document(&FakeSigner)).unwrap())
                .unwrap();
        assert_eq!(waiting["manuallyApprovesFollowers"], true);
        assert_eq!(document["manuallyApprovesFollowers"], false);
        assert_eq!(
            pollster::block_on(reached("BAD").document(&FakeSigner))
                .unwrap_err()
                .to_string(),
            "the secret BAD is not a key for this signature; give a PKCS#8 private key as PEM"
        );
        let finger: serde_json::Value = serde_json::from_str(&actor.finger()).unwrap();
        assert_eq!(finger, webfinger(DOMAIN, DOMAIN, "php-mutation-gate"));
        let announcement = announcement();
        let published: serde_json::Value =
            serde_json::from_str(&actor.note(&announcement)).unwrap();
        assert_eq!(
            published,
            note(
                DOMAIN,
                &announcement,
                &ActorRouteOptions {
                    content_warning: Some("Release notes".to_owned()),
                    ..plain_route()
                }
            )
        );
    }

    #[test]
    fn an_unlisted_note_is_addressed_to_followers_and_a_language_maps_its_content() {
        let shown = announcement();
        let public = note(DOMAIN, &shown, &plain_route());
        let followers = format!("{ACTOR}/followers");
        let everyone = "https://www.w3.org/ns/activitystreams#Public";
        assert_eq!(
            (&public["to"], &public["cc"]),
            (
                &serde_json::json!([everyone]),
                &serde_json::json!([followers])
            )
        );
        assert!(public.get("contentMap").is_none());
        let unlisted = ActorRouteOptions {
            visibility: herald_dest_fediverse::Audience::Unlisted,
            language: Some("nl".to_owned()),
            ..plain_route()
        };
        let quiet = note(DOMAIN, &shown, &unlisted);
        assert_eq!(
            (&quiet["to"], &quiet["cc"]),
            (
                &serde_json::json!([followers]),
                &serde_json::json!([everyone])
            )
        );
        assert_eq!(quiet["contentMap"]["nl"], quiet["content"]);
        let created = create(DOMAIN, &shown, &unlisted);
        assert_eq!(
            (&created["to"], &created["cc"]),
            (&quiet["to"], &quiet["cc"])
        );
        for language in ["english", "EN", "e"] {
            assert_eq!(
                ActorRouteOptions {
                    language: Some(language.to_owned()),
                    ..plain_route()
                }
                .check(),
                Err(format!(
                    "\"{language}\" is not an ISO 639 language code; give two or three lowercase letters, such as \"en\""
                ))
            );
        }
    }

    #[test]
    fn the_actor_owns_only_a_note_whose_id_is_its_own_note_of_that_key() {
        let actor = reached("HERALD_ACTIVITYPUB_KEY");
        let own = format!(r#"{{"id":"{ACTOR}/notes/k1"}}"#);
        assert!(actor.owns(&own, "k1"));
        assert!(!actor.owns(&own, "k2"));
        assert!(!actor.owns(
            r#"{"id":"https://herald.example/users/other/notes/k1"}"#,
            "k1"
        ));
        assert!(!actor.owns(r#"{"type":"Note"}"#, "k1"));
        assert!(!actor.owns("not json", "k1"));
    }

    #[test]
    fn a_note_keeps_every_web_link_as_a_link_matrix_s_pills_included() {
        let note = herald_core::convert(
            "[the room](https://matrix.to/#/#herald:example.org)",
            herald_dest_fediverse::ACTOR_MARKUP,
        );
        assert_eq!(
            note.html.as_deref(),
            Some(
                "<p>the room (<a href=\"https://matrix.to/#/#herald:example.org\">https://matrix.to/#\u{200b}/#\u{200b}herald:example.org</a>)</p>"
            )
        );
    }
}
