//! The fediverse kinds as herald reaches them: a Mastodon account and
//! herald's own actors read from their tables, and an actor serving its
//! outbox and followers and answering who follows it.

#[cfg(test)]
mod federates {
    use herald_core::{ReadBack, SecretRef, Symbols, WaitOptions, Waits};
    use herald_dest_fediverse::{
        ACTIVITYPUB, ActivityPub, ActorKind, ActorOptions, ActorRouteOptions, Approve, Audience,
        DestinationOptions, FIRST, FollowerCaps, MASTODON, Mastodon, MastodonKind, Mode, Policy,
        RouteOptions, actor_profile, profile,
    };
    use herald_kinds::{
        Activity, DestinationConfig, DestinationKind, Fault, Federates, Inboxes, Placed, Placing,
        Reach, Reached,
    };
    use herald_ports::{Base, Secret};
    use herald_testing::sample;
    use serde_json::{Value, json};

    const DOMAIN: &str = "herald.example";
    const ACTOR: &str = "https://herald.example/users/gate";
    const ANA: &str = "https://social.example/users/ana";

    fn key() -> Secret {
        Secret::new("HERALD_ACTIVITYPUB_KEY", "the-key")
    }

    fn route(actor: &str) -> ActorRouteOptions {
        ActorRouteOptions {
            actor: actor.to_owned(),
            content_warning: None,
            visibility: Audience::Public,
            language: None,
        }
    }

    fn actor() -> ActorOptions {
        serde_json::from_value(json!({
            "domain": DOMAIN,
            "key": { "secret": "HERALD_ACTIVITYPUB_KEY" },
            "symbols": { "bullet": "*" },
            "waits": { "outage": "7s" },
            "outbox_page": 3,
            "block": ["spam.example"],
            "followers": { "most": 2 },
            "approve": "manual",
        }))
        .unwrap()
    }

    fn mastodon() -> DestinationOptions {
        serde_json::from_value(json!({
            "server": "https://mastodon.example",
            "token": { "secret": "HERALD_MASTODON_TOKEN" },
            "symbols": { "bullet": "*" },
            "waits": { "outage": "7s" },
        }))
        .unwrap()
    }

    fn names(refs: &[&SecretRef]) -> Vec<String> {
        refs.iter().map(|named| named.secret.clone()).collect()
    }

    fn placing() -> Placing<'static> {
        Placing {
            destination: "social",
            subscribers: false,
        }
    }

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn gate() -> ActivityPub {
        ActivityPub::new(DOMAIN, "", key(), route("gate"))
    }

    #[test]
    fn a_mastodon_destination_reads_as_its_account_waiting_as_mastodon_does() {
        let options = mastodon();
        assert_eq!(options.kind(), MASTODON);
        assert_eq!(options.symbols_written(), options.symbols);
        assert_ne!(options.symbols_written(), Some(Symbols::default()));
        assert_eq!(options.waits_written(), options.waits);
        assert_ne!(options.waits_written(), Some(WaitOptions::default()));
        assert_eq!(options.platform_waits(), Mastodon::WAITS);
        assert_ne!(Mastodon::WAITS, Waits::default());
        assert_eq!(
            options.credential().map(|named| named.secret.as_str()),
            Some("HERALD_MASTODON_TOKEN")
        );
        assert_eq!(names(&options.secret_refs()), ["HERALD_MASTODON_TOKEN"]);
        let routed = RouteOptions::default();
        let placed = Placed::new(routed.clone());
        assert_eq!(options.profile(&placed), Some((profile(&routed), None)));
        assert_eq!(options.profile(&Placed::new(route("gate"))), None);
        let secret = Secret::new("HERALD_MASTODON_TOKEN", "token");
        let reached = options.reach(&placed, "", secret.clone());
        assert_eq!(
            reached,
            Some(Reached::new(
                Mastodon::new("https://mastodon.example", secret, routed)
                    .reading_back(ReadBack::DEFAULT)
                    .waiting(options.waits())
            ))
        );
        assert_eq!(
            reached.as_ref().map(|reached| reached.base()),
            Some(Base::At("https://mastodon.example"))
        );
    }

    #[test]
    fn a_route_to_mastodon_or_an_actor_is_refused_for_what_it_writes_wrongly() {
        let unwritten = RouteOptions {
            language: Some("English".to_owned()),
            ..RouteOptions::default()
        };
        assert_eq!(
            MastodonKind::placed(&mastodon(), &unwritten, placing()).len(),
            1
        );
        assert_eq!(
            MastodonKind::placed(&mastodon(), &RouteOptions::default(), placing()),
            []
        );
        assert_eq!(
            ActorKind::placed(&actor(), &route("-gate"), placing()),
            [Fault::here(
                "\"-gate\" is not an actor's name; give at most 30 lowercase letters, digits, `_` or a `-` within, such as \"php-mutation-gate\""
            )]
        );
        assert_eq!(ActorKind::placed(&actor(), &route("gate"), placing()), []);
    }

    #[test]
    fn an_actor_reads_as_one_of_herald_s_own_signing_with_its_key() {
        let options = actor();
        assert_eq!(options.kind(), ACTIVITYPUB);
        assert!(options.acts());
        assert_eq!(options.symbols_written(), options.symbols);
        assert_ne!(options.symbols_written(), Some(Symbols::default()));
        assert_eq!(options.waits_written(), options.waits);
        assert_ne!(options.waits_written(), Some(WaitOptions::default()));
        assert_eq!(
            options.credential().map(|named| named.secret.as_str()),
            Some("HERALD_ACTIVITYPUB_KEY")
        );
        assert_eq!(names(&options.secret_refs()), ["HERALD_ACTIVITYPUB_KEY"]);
        let placed = Placed::new(route("gate"));
        assert_eq!(options.profile(&placed), Some((actor_profile(), None)));
        assert_eq!(options.profile(&Placed::new(RouteOptions::default())), None);
        let reached = options.reach(&placed, "/herald", key()).unwrap();
        assert_eq!(
            reached,
            Reached::new(
                ActivityPub::new(DOMAIN, "/herald", key(), route("gate"))
                    .waiting(options.waits())
                    .paging(3)
                    .governed(options.policy())
            )
        );
        let served = reached.of::<ActivityPub>().unwrap();
        assert_eq!(served.outbox_most(), 3);
        assert!(served.policy().manual());
    }

    #[test]
    fn an_actor_blocks_what_its_lists_name_and_is_refused_where_one_cannot_be_read() {
        let csv = "listed.example,suspend\n";
        let read = |file: &str| {
            if file == "blocks.csv" {
                Ok(csv.to_owned())
            } else {
                Err("no such file".to_owned())
            }
        };
        let mut listed = actor();
        listed.lists = serde_json::from_value(json!([
            { "file": "blocks.csv", "sha256": herald_core::digest(csv) },
        ]))
        .unwrap();
        let (checked, faults) = ActorKind::checked(listed.clone(), &read).unwrap();
        assert_eq!(faults, []);
        assert_eq!(checked.block, ["spam.example", "listed.example"]);
        listed.lists = serde_json::from_value(json!([
            { "file": "blocks.csv", "sha256": herald_core::digest(csv) },
            { "file": "gone.csv", "sha256": "00" },
        ]))
        .unwrap();
        assert_eq!(
            ActorKind::checked(listed, &read).map(|_| ()),
            Err(Fault::at(
                "lists[1]",
                "gone.csv could not be read: no such file"
            ))
        );
        let mut unsound = actor();
        unsound.domain = "https://herald.example".to_owned();
        assert_eq!(
            ActorKind::checked(unsound, &nothing)
                .map(|_| ())
                .map_err(|fault| fault.key),
            Err(None)
        );
    }

    #[test]
    fn an_actor_is_reached_publicly_and_follows_publishes_and_federates_as_itself() {
        let gate = gate();
        assert_eq!(gate.base(), Base::Public);
        assert_eq!(gate.follows(), Some("gate"));
        let announcement = sample::announcement();
        assert_eq!(
            gate.published(&announcement),
            Some(("gate", gate.note(&announcement)))
        );
        let governed = gate.clone().governed(Policy::new(
            Mode::Open,
            &["spam.example".to_owned()],
            &[],
            FollowerCaps::default(),
        ));
        assert!(Reach::admits(&governed, ANA));
        assert!(!Reach::admits(&governed, "https://spam.example/users/x"));
        assert_eq!(gate.federates().map(Federates::id), Some(ACTOR.to_owned()));
    }

    #[test]
    fn an_actor_federates_under_its_own_names() {
        let gate = gate();
        assert_eq!(Federates::actor(&gate), "gate");
        assert_eq!(Federates::domain(&gate), DOMAIN);
        assert_eq!(Federates::id(&gate), ACTOR);
        assert_eq!(Federates::finger(&gate), gate.finger());
        let finger: Value = serde_json::from_str(&Federates::finger(&gate)).unwrap();
        assert_eq!(finger["subject"], "acct:gate@herald.example");
        assert_eq!(Federates::first_page(&gate), FIRST);
        assert_eq!(FIRST, "true");
        assert_eq!(Federates::outbox_most(&gate), ActorOptions::OUTBOX_PAGE);
        assert_eq!(Federates::outbox_most(&gate.clone().paging(7)), 7);
        assert_eq!(Federates::deliveries(&gate), ActorOptions::DELIVERIES);
        assert_eq!(Federates::deliveries(&gate.clone().delivering(3)), 3);
        let announcement = sample::announcement();
        let note = gate.note(&announcement);
        let key = announcement.key.to_string();
        assert!(Federates::owns(&gate, &note, &key));
        assert!(!Federates::owns(&gate, &note, "another"));
    }

    #[test]
    fn an_outbox_counts_its_notes_and_pages_the_actor_s_own_newest_first() {
        let gate = gate();
        let outbox: Value = serde_json::from_str(&Federates::outbox(&gate, 4)).unwrap();
        assert_eq!(
            outbox,
            json!({
                "@context": "https://www.w3.org/ns/activitystreams",
                "id": format!("{ACTOR}/outbox"),
                "type": "OrderedCollection",
                "totalItems": 4,
                "first": format!("{ACTOR}/outbox?page=true"),
            })
        );
        let own = gate.note(&sample::announcement());
        let other =
            ActivityPub::new(DOMAIN, "", key(), route("other")).note(&sample::announcement());
        let notes = [own.clone(), other, "not json".to_owned()];
        let page: Value =
            serde_json::from_str(&Federates::outbox_page(&gate, FIRST, &notes, Some("2"))).unwrap();
        assert_eq!(page["id"], format!("{ACTOR}/outbox?page=true"));
        assert_eq!(page["type"], "OrderedCollectionPage");
        assert_eq!(page["partOf"], format!("{ACTOR}/outbox"));
        assert_eq!(page["next"], format!("{ACTOR}/outbox?page=2"));
        let items = page["orderedItems"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["type"], "Create");
        assert_eq!(
            items[0]["object"],
            serde_json::from_str::<Value>(&own).unwrap()
        );
        let last: Value =
            serde_json::from_str(&Federates::outbox_page(&gate, "2", &[], None)).unwrap();
        assert_eq!(last["id"], format!("{ACTOR}/outbox?page=2"));
        assert_eq!(last["orderedItems"], json!([]));
        assert!(last.get("next").is_none());
    }

    #[test]
    fn a_followers_collection_counts_followers_and_never_names_them() {
        let followers: Value = serde_json::from_str(&Federates::followers(&gate(), 2)).unwrap();
        assert_eq!(
            followers,
            json!({
                "@context": "https://www.w3.org/ns/activitystreams",
                "id": format!("{ACTOR}/followers"),
                "type": "OrderedCollection",
                "totalItems": 2,
            })
        );
    }

    #[test]
    fn a_follower_is_admitted_given_room_and_waits_where_approval_is_manual() {
        let options = actor();
        let gate = gate().governed(options.policy());
        assert!(Federates::admits(&gate, ANA));
        assert!(!Federates::admits(&gate, "https://spam.example/users/x"));
        assert!(Federates::room(&gate, ANA, &[]));
        assert!(!Federates::room(
            &gate,
            ANA,
            &["https://a.example/users/1", "https://b.example/users/2"]
        ));
        assert!(Federates::manual(&gate));
        let automatic = gate
            .clone()
            .governed(Policy::default().approving(Approve::Auto));
        assert!(!Federates::manual(&automatic));
    }

    #[test]
    fn a_follow_or_its_undoing_is_read_and_anything_else_says_why_not() {
        let gate = gate();
        let follow = json!({ "id": "https://social.example/f/1", "type": "Follow", "actor": ANA, "object": ACTOR });
        assert_eq!(
            Federates::read(&gate, follow.to_string().as_bytes(), ANA),
            Activity::Follow {
                target: ACTOR.to_owned(),
                id: "https://social.example/f/1".to_owned(),
            }
        );
        let undo = json!({ "type": "Undo", "actor": ANA, "object": { "type": "Follow", "actor": ANA, "object": ACTOR } });
        assert_eq!(
            Federates::read(&gate, undo.to_string().as_bytes(), ANA),
            Activity::Undo {
                follower: ANA.to_owned(),
                target: ACTOR.to_owned(),
            }
        );
        assert_eq!(
            Federates::read(&gate, b"{", ANA),
            Activity::Nothing("it is not JSON".to_owned())
        );
    }

    #[test]
    fn a_follower_s_inboxes_are_read_from_its_own_document() {
        let gate = gate();
        let person = json!({
            "id": ANA,
            "type": "Person",
            "inbox": "https://social.example/users/ana/inbox",
            "endpoints": { "sharedInbox": "https://social.example/inbox" },
        });
        assert_eq!(
            Federates::inboxes(&gate, person.to_string().as_bytes(), ANA),
            Some(Inboxes {
                personal: "https://social.example/users/ana/inbox".to_owned(),
                delivered: "https://social.example/inbox".to_owned(),
            })
        );
        assert_eq!(Federates::inboxes(&gate, b"not json", ANA), None);
    }

    #[test]
    fn a_waiting_follow_is_accepted_or_rejected_as_the_follow_it_was() {
        let gate = gate();
        let follow = gate.follow_of("https://social.example/f/1", ANA);
        assert_eq!(
            follow,
            json!({
                "id": "https://social.example/f/1",
                "type": "Follow",
                "actor": ANA,
                "object": ACTOR,
            })
        );
        let rejected = gate.reject(&follow);
        assert_eq!(rejected["type"], "Reject");
        assert_eq!(rejected["actor"], ACTOR);
        assert_eq!(rejected["object"], follow);
        let id = rejected["id"].as_str().unwrap();
        assert!(id.starts_with(&format!("{ACTOR}/rejects/")), "{id}");
        assert_eq!(
            serde_json::from_str::<Value>(&Federates::answer(
                &gate,
                "https://social.example/f/1",
                ANA,
                true
            ))
            .unwrap(),
            gate.accept(&follow)
        );
        assert_eq!(
            serde_json::from_str::<Value>(&Federates::answer(
                &gate,
                "https://social.example/f/1",
                ANA,
                false
            ))
            .unwrap(),
            rejected
        );
        assert_eq!(
            herald_dest_fediverse::follow_id(&follow),
            "https://social.example/f/1"
        );
        assert_eq!(herald_dest_fediverse::follow_id(&json!({})), "");
    }
}
