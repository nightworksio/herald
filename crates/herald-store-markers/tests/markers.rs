//! The markers store over fakes of a destination's posts, through the
//! conformance suite every store passes.

#[cfg(test)]
mod markers {
    use herald_core::{Chore, Entry, Identity, Key, Marker, RouteId, Schedule, Step, Thread};
    use herald_ports::{MarkedThread, Secret, Store, StoreError, Version, Versioned, marker};
    use herald_store_markers::{MarkedRoute, MarkersStore};
    use herald_testing::{FakeMarkers, FakeSigner, store_conformance};
    use jiff::{SignedDuration, Timestamp};

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
            pollster::block_on(store.entry(&key)),
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
        };
        assert_eq!(
            pollster::block_on(store.put_cursor(&SourceName::new("s"), &Cursor::default(), None)),
            Err(unsupported("cursors"))
        );
        assert_eq!(
            pollster::block_on(store.subscribers(&DestinationName::new("discord"), "p")),
            Err(unsupported("subscriptions"))
        );
        assert_eq!(
            pollster::block_on(store.unsubscribe(&subscription)),
            Err(unsupported("subscriptions"))
        );
        assert_eq!(
            pollster::block_on(store.put_pinned(&RouteId::new("r"), "1")),
            Err(unsupported("pins"))
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
            name: "{{ event.version }}".to_owned(),
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
            pollster::block_on(store.schedules()),
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
            pollster::block_on(unreadable.schedules()),
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
