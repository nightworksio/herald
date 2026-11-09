use super::*;

struct Derived;

impl Ledger for Derived {
    fn keeping(&self) -> Keeping {
        Keeping::Derived
    }

    fn entry(
        &self,
        _: &Key,
    ) -> impl Future<Output = Result<Option<Versioned<Entry>>, StoreError>> + Send {
        ready(Ok(None))
    }

    fn put_entry(
        &self,
        _: &Entry,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        ready(Ok(Written::Written(Version(7))))
    }

    fn expire_entries(
        &self,
        _: Timestamp,
    ) -> impl Future<Output = Result<Vec<Key>, StoreError>> + Send {
        ready(Ok(vec![Key::derive(
            &RouteId::new("r"),
            &herald_core::Identity::new("1"),
        )]))
    }
}

impl Schedules for Derived {
    fn keeping(&self) -> Keeping {
        Keeping::Derived
    }

    fn every_schedule(&self) -> impl Future<Output = Result<Vec<Schedule>, StoreError>> + Send {
        ready(Ok(Vec::new()))
    }

    fn put_schedules(
        &self,
        _: &[Schedule],
        _: &[Schedule],
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        ready(Ok(()))
    }
}

fn unsupported<T>(answer: impl Future<Output = Result<T, StoreError>>) -> Result<(), StoreError> {
    pollster::block_on(answer).map(drop)
}

const fn naming(what: &'static str) -> Result<(), StoreError> {
    Err(StoreError::Unsupported { what })
}

fn itself<T>(answered: Option<&T>, store: &T) -> bool {
    answered.is_some_and(|kept| std::ptr::eq(kept, store))
}

impl Keeper for Option<&'static Unkept> {
    fn holds(&self, _: Held) -> bool {
        self.is_some()
    }

    fn skipped(&self) -> Vec<String> {
        vec!["corrupt.json".to_owned()]
    }
}

#[test]
fn a_keeper_answers_each_kind_it_holds_itself_and_none_it_does_not() {
    let holding = Some(&Unkept);
    let held = |answered| itself(answered, &holding);
    assert!(held(holding.ledger()) && held(holding.cursors()));
    assert!(held(holding.schedules()) && held(holding.subscriptions()));
    assert!(held(holding.pins()) && held(holding.deliveries()));
    assert!(held(holding.notes()) && held(holding.announcements()));
    assert!(held(holding.inboxes()));
    assert_eq!(Store::skipped(&holding), ["corrupt.json"]);
    let none = None::<&Unkept>;
    assert!(none.ledger().is_none() && none.cursors().is_none());
    assert!(none.schedules().is_none() && none.subscriptions().is_none());
    assert!(none.pins().is_none() && none.deliveries().is_none());
    assert!(none.notes().is_none() && none.announcements().is_none());
    assert!(none.inboxes().is_none());
}

#[test]
fn a_kind_kept_somewhere_answers_as_its_keeper_does() {
    let kept = Some(&Derived);
    assert_eq!(Ledger::keeping(&kept), Keeping::Derived);
    assert_eq!(Schedules::keeping(&kept), Keeping::Derived);
    let key = Key::derive(&RouteId::new("r"), &herald_core::Identity::new("1"));
    assert_eq!(pollster::block_on(kept.entry(&key)), Ok(None));
    assert_eq!(
        pollster::block_on(kept.put_entry(&Entry::new(key), None)),
        Ok(Written::Written(Version(7)))
    );
    assert_eq!(
        pollster::block_on(kept.expire_entries(Timestamp::UNIX_EPOCH)),
        Ok(vec![key])
    );
    assert_eq!(pollster::block_on(kept.every_schedule()), Ok(Vec::new()));
    assert_eq!(pollster::block_on(kept.put_schedules(&[], &[])), Ok(()));
}

#[test]
fn the_run_s_kinds_kept_nowhere_answer_unsupported_naming_them() {
    let key = Key::derive(&RouteId::new("r"), &herald_core::Identity::new("1"));
    let source = SourceName::new("s");
    let route = RouteId::new("r");
    let destination = DestinationName::new("d");
    let subscription = Subscription {
        destination: destination.clone(),
        project: "p".to_owned(),
        subscriber: "s".to_owned(),
        inbox: None,
        pending: None,
    };
    for unkept in [None, Some(&Unkept)] {
        assert_eq!(unsupported(unkept.entry(&key)), naming("ledger"));
        assert_eq!(
            unsupported(unkept.put_entry(&Entry::new(key), None)),
            naming("ledger")
        );
        assert_eq!(
            unsupported(unkept.expire_entries(Timestamp::UNIX_EPOCH)),
            naming("ledger")
        );
        assert_eq!(unsupported(unkept.cursor(&source)), naming("cursors"));
        assert_eq!(
            unsupported(unkept.put_cursor(&source, &Cursor::default(), None)),
            naming("cursors")
        );
        assert_eq!(unsupported(unkept.every_schedule()), naming("schedules"));
        assert_eq!(
            unsupported(unkept.put_schedules(&[], &[])),
            naming("schedules")
        );
        assert_eq!(
            unsupported(unkept.subscribers(&destination, "p")),
            naming("subscriptions")
        );
        assert_eq!(
            unsupported(unkept.subscribe(&subscription)),
            naming("subscriptions")
        );
        assert_eq!(
            unsupported(unkept.unsubscribe(&subscription)),
            naming("subscriptions")
        );
        assert_eq!(unsupported(unkept.pinned(&route)), naming("pins"));
        assert_eq!(unsupported(unkept.put_pinned(&route, "1")), naming("pins"));
    }
}

#[test]
fn the_served_kinds_kept_nowhere_answer_unsupported_naming_them() {
    let key = Key::derive(&RouteId::new("r"), &herald_core::Identity::new("1"));
    let state = State::Attempted {
        at: Timestamp::UNIX_EPOCH,
    };
    let at = Timestamp::UNIX_EPOCH;
    let latest = Latest {
        project: "p".to_owned(),
        newest: herald_core::Shown {
            kind: herald_core::Kind::Release,
            version: None,
            title: "t".to_owned(),
            body: String::new(),
            links: std::collections::BTreeMap::new(),
            prerelease: false,
            occurred: at,
        },
        stable: None,
        routes: std::collections::BTreeSet::new(),
    };
    for unkept in [None, Some(&Unkept)] {
        assert_eq!(
            unsupported(unkept.admit(Scheme::GitHub, "1", at)),
            naming("deliveries")
        );
        assert_eq!(
            unsupported(unkept.release(Scheme::GitHub, "1")),
            naming("deliveries")
        );
        assert_eq!(
            unsupported(unkept.expire_deliveries(at)),
            naming("deliveries")
        );
        assert_eq!(unsupported(unkept.note(&key)), naming("notes"));
        assert_eq!(
            unsupported(unkept.put_note("herald", &key, "{}", at)),
            naming("notes")
        );
        assert_eq!(
            unsupported(unkept.outbox("herald", None, 1)),
            naming("notes")
        );
        assert_eq!(unsupported(unkept.expire_notes(at)), naming("notes"));
        assert_eq!(unsupported(unkept.latest("p")), naming("latest"));
        assert_eq!(unsupported(unkept.every_latest()), naming("latest"));
        assert_eq!(
            unsupported(unkept.put_latest(&latest, None)),
            naming("latest")
        );
        assert_eq!(
            unsupported(unkept.delivery(&key, "i")),
            naming("deliveries to followers")
        );
        assert_eq!(
            unsupported(unkept.put_delivery(&key, "i", &state)),
            naming("deliveries to followers")
        );
        assert_eq!(
            unsupported(unkept.forget_deliveries(&key)),
            naming("deliveries to followers")
        );
        assert_eq!(Ledger::keeping(&unkept), Keeping::Kept);
    }
}

#[test]
fn every_store_error_reads_as_a_sentence() {
    assert_eq!(
        StoreError::Unavailable {
            reason: "down".to_owned()
        }
        .to_string(),
        "the store could not be reached: down"
    );
    assert_eq!(
        StoreError::Corrupt {
            reason: "bad json".to_owned()
        }
        .to_string(),
        "a record in the store could not be read: bad json"
    );
    assert_eq!(
        StoreError::Unsupported { what: "cursors" }.to_string(),
        "this store keeps no cursors; choose a store that does"
    );
}
