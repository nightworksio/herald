//! The markers store over fakes of a destination's posts, through the
//! conformance suite every store passes.

#[cfg(test)]
mod markers {
    use herald_core::{Chore, Entry, Identity, Key, Marker, RouteId, Schedule, Step, Thread};
    use herald_ports::{
        Announcements, Cursors, Deliveries, Ledger, MarkedThread, Notes, Pins, Schedules, Secret,
        Store, StoreError, Subscriptions, Version, Versioned, marker,
    };
    use herald_store_markers::{MarkedRoute, MarkersStore};
    use herald_testing::{FakeMarkers, FakeSigner, store_conformance};
    use jiff::{SignedDuration, Timestamp};

    /// A scheme as its crate writes it, for these tests.
    const GITHUB: herald_core::Scheme = herald_core::Scheme {
        name: "github",
        signed: herald_core::Signed::Body {
            headers: &["X-Hub-Signature-256"],
            prefix: "sha256=",
        },
        key: herald_core::KeyForm::Bytes,
    };

    const OPENED: Timestamp = Timestamp::constant(1_793_448_000, 0);
    const DAY: SignedDuration = SignedDuration::from_hours(24);

    fn route(name: &str, markers: FakeMarkers, thread: Option<Thread>) -> MarkedRoute<FakeMarkers> {
        MarkedRoute {
            route: RouteId::new(name),
            thread,
            markers,
            key: Secret::new(format!("the marker key of {name}"), name),
        }
    }

    fn mark(secret: &str, message: &[u8]) -> Marker {
        pollster::block_on(marker(&FakeSigner, &Secret::new("K", secret), message)).unwrap()
    }

    fn key(identity: &str) -> Key {
        Key::derive(&RouteId::new("releases"), &Identity::new(identity))
    }

    #[test]
    fn the_store_passes_the_conformance_suite() {
        let store = MarkersStore::new(
            vec![route("releases", FakeMarkers::default(), None)],
            FakeSigner,
        );
        assert_eq!(pollster::block_on(store_conformance(&store)), Ok(()));
    }

    #[test]
    fn an_entry_is_what_herald_s_post_under_any_route_s_marker_shows() {
        let key = key("v1");
        let landed = Entry::new(key).landed(Step::Post, herald_core::Facts::default());
        let carried = mark("releases", &Marker::message(&key));
        let store = MarkersStore::new(
            vec![
                route("other", FakeMarkers::default(), None),
                route(
                    "releases",
                    FakeMarkers::default().with(landed.clone(), carried),
                    None,
                ),
            ],
            FakeSigner,
        );
        assert_eq!(
            pollster::block_on(store.ledger().entry(&key)),
            Ok(Some(Versioned {
                value: landed,
                version: Version(1),
            }))
        );
        let guessed = Entry::new(key);
        let forged = MarkersStore::new(
            vec![route(
                "releases",
                FakeMarkers::default().with(guessed, mark("guessed", &Marker::message(&key))),
                None,
            )],
            FakeSigner,
        );
        assert_eq!(pollster::block_on(forged.entry(&key)), Ok(None));
    }

    #[test]
    fn what_a_post_cannot_hold_is_unsupported() {
        use herald_core::{Cursor, DestinationName, SourceName, Subscription};
        let store = MarkersStore::new(Vec::<MarkedRoute<FakeMarkers>>::new(), FakeSigner);
        let unsupported = |what| StoreError::Unsupported { what };
        let subscription = Subscription {
            destination: DestinationName::new("discord"),
            project: "p".to_owned(),
            subscriber: "s".to_owned(),
            inbox: None,
            pending: None,
        };
        assert_eq!(
            pollster::block_on(store.cursors().put_cursor(
                &SourceName::new("s"),
                &Cursor::default(),
                None
            )),
            Err(unsupported("cursors"))
        );
        assert_eq!(
            pollster::block_on(
                store
                    .subscriptions()
                    .subscribers(&DestinationName::new("discord"), "p")
            ),
            Err(unsupported("subscriptions"))
        );
        assert_eq!(
            pollster::block_on(store.subscriptions().unsubscribe(&subscription)),
            Err(unsupported("subscriptions"))
        );
        assert_eq!(
            pollster::block_on(store.pins().put_pinned(&RouteId::new("r"), "1")),
            Err(unsupported("pins"))
        );
        assert_eq!(
            pollster::block_on(store.deliveries().release(GITHUB, "d")),
            Err(unsupported("deliveries"))
        );
        assert_eq!(
            pollster::block_on(store.deliveries().expire_deliveries(OPENED)),
            Err(unsupported("deliveries"))
        );
        let key = Key::derive(&RouteId::new("r"), &Identity::new("1"));
        assert_eq!(
            pollster::block_on(store.notes().put_note("herald", &key, "{}", OPENED)),
            Err(unsupported("notes"))
        );
        assert_eq!(
            pollster::block_on(store.notes().outbox("herald", None, 1)),
            Err(unsupported("notes"))
        );
        assert_eq!(
            pollster::block_on(store.ledger().expire_entries(OPENED)),
            Ok(Vec::new())
        );
        assert_eq!(
            pollster::block_on(store.announcements().latest("herald")),
            Err(unsupported("latest"))
        );
        assert_eq!(
            pollster::block_on(store.announcements().every_latest()),
            Err(unsupported("latest"))
        );
        let latest = herald_core::Latest {
            project: "herald".to_owned(),
            newest: herald_core::Shown {
                kind: herald_core::Kind::RELEASE,
                attributes: std::collections::BTreeMap::default(),
                title: "t".to_owned(),
                body: String::new(),
                links: std::collections::BTreeMap::new(),
                occurred: OPENED,
            },
            stable: None,
            routes: std::collections::BTreeSet::new(),
        };
        assert_eq!(
            pollster::block_on(store.announcements().put_latest(&latest, None)),
            Err(unsupported("latest"))
        );
    }

    #[test]
    fn neither_keys_nor_deliveries_to_followers_are_kept_by_posts() {
        let store = MarkersStore::new(Vec::<MarkedRoute<FakeMarkers>>::new(), FakeSigner);
        let unsupported = |what| StoreError::Unsupported { what };
        let key = Key::derive(&RouteId::new("r"), &Identity::new("1"));
        let known = herald_ports::KnownKey {
            owner: "o".to_owned(),
            pem: "p".to_owned(),
            fetched: OPENED,
        };
        assert_eq!(
            pollster::block_on(herald_ports::Keyring::known(&store, "k")),
            Err(unsupported("keys"))
        );
        assert_eq!(
            pollster::block_on(herald_ports::Keyring::know(&store, "k", &known)),
            Err(unsupported("keys"))
        );
        assert_eq!(
            pollster::block_on(herald_ports::Keyring::forget_before(&store, OPENED)),
            Err(unsupported("keys"))
        );
        let inbox = "https://social.example/inbox";
        assert_eq!(
            pollster::block_on(herald_ports::Inboxes::delivery(
                &store.inboxes(),
                &key,
                inbox
            )),
            Err(unsupported("deliveries to followers"))
        );
        let attempted = herald_core::State::Attempted { at: OPENED };
        assert_eq!(
            pollster::block_on(herald_ports::Inboxes::put_delivery(
                &store.inboxes(),
                &key,
                inbox,
                &attempted
            )),
            Err(unsupported("deliveries to followers"))
        );
    }

    fn opened(thread: &str, archived: bool, locked: bool) -> MarkedThread {
        MarkedThread {
            thread: thread.to_owned(),
            opened: OPENED,
            archived,
            locked,
        }
    }

    #[test]
    fn each_thread_on_a_route_falls_due_for_the_chores_it_has_not_had() {
        let carried = mark(
            "releases",
            &Marker::route_message(&RouteId::new("releases")),
        );
        let markers = FakeMarkers::default()
            .with_thread(carried.clone(), opened("1", false, false))
            .with_thread(carried.clone(), opened("2", true, false))
            .with_thread(carried.clone(), opened("3", false, true))
            .with_thread(carried, opened("4", true, true));
        let thread = Thread {
            name: "{{ event.attributes.version }}".to_owned(),
            archive_after: Some(DAY),
            lock_after: Some(DAY * 7),
        };
        let store = MarkersStore::new(
            vec![
                route("releases", markers, Some(thread)),
                route("unthreaded", FakeMarkers::default(), None),
            ],
            FakeSigner,
        );
        let schedule = |id: &str, chore, after| Schedule {
            key: Key::derive(
                &RouteId::new("releases"),
                &Identity::new(format!("thread/{id}")),
            ),
            route: RouteId::new("releases"),
            thread: id.to_owned(),
            chore,
            due: OPENED.checked_add(after).unwrap(),
        };
        assert_eq!(
            pollster::block_on(store.schedules().every_schedule()),
            Ok(vec![
                schedule("1", Chore::Archive, DAY),
                schedule("1", Chore::Lock, DAY * 7),
                schedule("2", Chore::Lock, DAY * 7),
                schedule("3", Chore::Archive, DAY),
            ])
        );
    }

    #[test]
    fn posts_that_cannot_be_read_or_a_key_that_cannot_mark_leave_the_store_unavailable() {
        let thread = Thread {
            name: "t".to_owned(),
            archive_after: Some(DAY),
            lock_after: None,
        };
        let unreadable = MarkersStore::new(
            vec![route("releases", FakeMarkers::unreadable(), Some(thread))],
            FakeSigner,
        );
        let reason =
            "the destination's posts could not be read: the fake destination is unreadable";
        let unavailable = Err(StoreError::Unavailable {
            reason: reason.to_owned(),
        });
        assert_eq!(
            pollster::block_on(unreadable.entry(&key("v1"))),
            unavailable
        );
        assert_eq!(
            pollster::block_on(unreadable.every_schedule()),
            unavailable.map(|_| Vec::new())
        );
        let mut bad = route("releases", FakeMarkers::default(), None);
        bad.key = Secret::new("BAD", "x");
        let unmarked = MarkersStore::new(vec![bad], FakeSigner);
        assert!(matches!(
            pollster::block_on(unmarked.entry(&key("v1"))),
            Err(StoreError::Unavailable { .. })
        ));
    }
}
