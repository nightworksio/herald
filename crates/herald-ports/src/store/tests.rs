use super::*;

const GITHUB: &str = "github";

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
    assert!(held(holding.inboxes()) && held(holding.positions()));
    assert!(held(holding.waiting()) && held(holding.decisions()));
    assert!(held(holding.pulls()) && held(holding.recalls()));
    assert!(held(holding.actions()));
    assert_eq!(Store::skipped(&holding), ["corrupt.json"]);
    assert!(!Store::writes_one_at_a_time(&holding));
    let none = None::<&Unkept>;
    assert!(none.ledger().is_none() && none.cursors().is_none());
    assert!(none.schedules().is_none() && none.subscriptions().is_none());
    assert!(none.pins().is_none() && none.deliveries().is_none());
    assert!(none.notes().is_none() && none.announcements().is_none());
    assert!(none.inboxes().is_none() && none.positions().is_none());
    assert!(none.waiting().is_none() && none.decisions().is_none());
    assert!(none.pulls().is_none() && none.recalls().is_none());
    assert!(none.actions().is_none());
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
        assert_eq!(
            unsupported(unkept.forget_pinned(&route, "1")),
            naming("pins")
        );
        assert_eq!(
            unsupported(unkept.position(&destination)),
            naming("session positions")
        );
        assert_eq!(
            unsupported(unkept.put_position(&destination, "s1")),
            naming("session positions")
        );
        let decided = herald_core::Decided {
            key,
            task: "t".to_owned(),
            subject: "s".to_owned(),
            answer: herald_core::Answer {
                model: "m".to_owned(),
                choice: None,
                probabilities: std::collections::BTreeMap::new(),
            },
            at: Timestamp::UNIX_EPOCH,
        };
        assert_eq!(unsupported(unkept.decision(&key)), naming("decisions"));
        assert_eq!(
            unsupported(unkept.put_decision(&decided)),
            naming("decisions")
        );
        assert_eq!(
            unsupported(unkept.expire_decisions(Timestamp::UNIX_EPOCH)),
            naming("decisions")
        );
        let hold = herald_core::Hold::Tag {
            source: SourceName::new("s"),
            identity: herald_core::Identity::new("1"),
        };
        let pending = herald_core::Pending {
            hold: hold.clone(),
            events: Vec::new(),
            opened: Timestamp::UNIX_EPOCH,
            due: Timestamp::UNIX_EPOCH,
            closed: None,
            landed: false,
        };
        assert_eq!(unsupported(unkept.pending(&hold)), naming("held events"));
        assert_eq!(unsupported(unkept.every_pending()), naming("held events"));
        assert_eq!(
            unsupported(unkept.put_pending(&pending, None)),
            naming("held events")
        );
        assert_eq!(
            unsupported(unkept.drop_pending(&hold)),
            naming("held events")
        );
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
            kind: herald_core::Kind::RELEASE,
            attributes: std::collections::BTreeMap::default(),
            title: "t".to_owned(),
            body: String::new(),
            links: std::collections::BTreeMap::new(),
            occurred: at,
            identity: None,
        },
        stable: None,
        routes: std::collections::BTreeSet::new(),
    };
    for unkept in [None, Some(&Unkept)] {
        assert_eq!(
            unsupported(unkept.admit(GITHUB, "1", at)),
            naming("deliveries")
        );
        assert_eq!(unsupported(unkept.seen(GITHUB, "1")), naming("deliveries"));
        assert_eq!(
            unsupported(unkept.release(GITHUB, "1")),
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
        assert_eq!(unsupported(unkept.forget_note(&key)), naming("notes"));
        assert_eq!(unsupported(unkept.latest("p")), naming("latest"));
        assert_eq!(unsupported(unkept.every_latest()), naming("latest"));
        assert_eq!(
            unsupported(unkept.put_latest(&latest, None)),
            naming("latest")
        );
        assert_eq!(unsupported(unkept.forget_latest("p")), naming("latest"));
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

#[test]
fn a_store_error_is_built_from_what_went_wrong() {
    assert_eq!(
        StoreError::unavailable(404),
        StoreError::Unavailable {
            reason: "404".to_owned()
        }
    );
    assert_eq!(
        StoreError::answered("GitHub", 502),
        StoreError::Unavailable {
            reason: "GitHub answered 502".to_owned()
        }
    );
    assert_eq!(
        StoreError::corrupt("bad json"),
        StoreError::Corrupt {
            reason: "bad json".to_owned()
        }
    );
    assert_eq!(
        StoreError::unreadable("ledger/a.json", "EOF"),
        StoreError::Corrupt {
            reason: "ledger/a.json: EOF".to_owned()
        }
    );
}

#[test]
fn an_outbox_page_holds_the_newest_notes_older_than_where_it_starts() {
    let listed = |names: &[&str]| -> Vec<(String, &'static str)> {
        names
            .iter()
            .map(|name| ((*name).to_owned(), "note"))
            .collect()
    };
    let named = |names: &[&str]| -> Vec<(String, String)> {
        names
            .iter()
            .map(|name| ((*name).to_owned(), (*name).to_owned()))
            .collect()
    };
    assert_eq!(
        Outboxed::page(named(&["1", "3", "2", "4"]), None, 2),
        Outboxed {
            total: 4,
            notes: vec!["4".to_owned(), "3".to_owned()],
            next: Some("3".to_owned()),
        }
    );
    assert_eq!(
        Outboxed::page(named(&["1", "3", "2", "4"]), Some("3"), 2),
        Outboxed {
            total: 4,
            notes: vec!["2".to_owned(), "1".to_owned()],
            next: None,
        }
    );
    assert_eq!(
        Outboxed::page(named(&["1", "2", "3"]), Some("2"), 1),
        Outboxed {
            total: 3,
            notes: vec!["1".to_owned()],
            next: None,
        }
    );
    assert_eq!(
        Outboxed::page(listed(&["1", "2"]), None, 2),
        Outboxed {
            total: 2,
            notes: vec!["note", "note"],
            next: None,
        }
    );
    assert_eq!(
        Outboxed::page(listed(&["1", "2", "3"]), Some("1"), 2),
        Outboxed {
            total: 3,
            notes: Vec::new(),
            next: None,
        }
    );
}

#[test]
fn an_outbox_names_a_note_by_when_it_was_published_and_its_key() {
    let key = Key::derive(&RouteId::new("r"), &herald_core::Identity::new("e"));
    let at = Timestamp::from_second(1_700_000_000).unwrap();
    let name = Outboxed::name(at, &key);
    assert_eq!(name, format!("01700000000000000000.{key}"));
    assert_eq!(Outboxed::key(&name), Some(key));
    assert_eq!(Outboxed::key("01700000000000000000"), None);
    assert_eq!(Outboxed::key("1.nope"), None);
    assert!(Outboxed::is_name(&name));
    assert!(Outboxed::name(Timestamp::UNIX_EPOCH, &key) < name);
    for unnamed in [
        format!("1700000000000000000.{key}"),
        format!("0170000000000000000x.{key}"),
        "01700000000000000000.nope".to_owned(),
        "01700000000000000000".to_owned(),
    ] {
        assert!(!Outboxed::is_name(&unnamed), "{unnamed}");
    }
}

#[test]
fn an_owner_source_s_records_kept_nowhere_answer_unsupported_naming_cursors() {
    let source = SourceName::new("s");
    let listing = herald_core::Listing {
        owner: herald_core::Owner::Organization,
        pages: Vec::new(),
        first: Timestamp::UNIX_EPOCH,
        listed: Timestamp::UNIX_EPOCH,
        fingerprint: "f".to_owned(),
        file: None,
        events: None,
        stale: false,
    };
    let membership = herald_core::Membership {
        repository: herald_core::Listed {
            id: "1".to_owned(),
            name: "n".to_owned(),
            path: "o/n".to_owned(),
            topics: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            visibility: herald_core::Visibility::Public,
            archived: false,
            fork: false,
            template: false,
            language: None,
            linked: None,
        },
        discovered: Timestamp::UNIX_EPOCH,
        left: None,
        cursor: None,
        collides: None,
        hooked: false,
    };
    for unkept in [None, Some(&Unkept)] {
        assert_eq!(unsupported(unkept.listing(&source)), naming("cursors"));
        assert_eq!(
            unsupported(unkept.put_listing(&source, &listing, None)),
            naming("cursors")
        );
        assert_eq!(unsupported(unkept.members(&source)), naming("cursors"));
        assert_eq!(unsupported(unkept.member(&source, "1")), naming("cursors"));
        assert_eq!(
            unsupported(unkept.put_member(&source, &membership, None)),
            naming("cursors")
        );
        assert_eq!(
            unsupported(unkept.forget_member(&source, "1")),
            naming("cursors")
        );
    }
}

#[test]
fn pull_requests_kept_nowhere_answer_unsupported_naming_them() {
    for unkept in [None, Some(&Unkept)] {
        let repository = herald_core::RepositoryKey {
            host: "github.com".to_owned(),
            id: "1".to_owned(),
        };
        let pull = herald_core::PullKey {
            repository: repository.clone(),
            number: 1,
        };
        let reconciled = herald_core::Reconciled {
            repositories: std::collections::BTreeSet::new(),
            repository: None,
            at: None,
            validators: Vec::new(),
        };
        let source = SourceName::new("s");
        assert_eq!(unsupported(unkept.pull(&pull)), naming("pull requests"));
        assert_eq!(
            unsupported(unkept.put_pull(&kept_pull(), None)),
            naming("pull requests")
        );
        assert_eq!(
            unsupported(unkept.pulls_of(&repository)),
            naming("pull requests")
        );
        assert_eq!(
            unsupported(unkept.expire_pulls(Timestamp::UNIX_EPOCH)),
            naming("pull requests")
        );
        assert_eq!(
            unsupported(unkept.reconciled(&source)),
            naming("pull requests")
        );
        assert_eq!(
            unsupported(unkept.put_reconciled(&source, &reconciled, None)),
            naming("pull requests")
        );
    }
}

#[test]
fn the_names_of_advised_reports_kept_nowhere_answer_unsupported_naming_them() {
    for unkept in [None, Some(&Unkept)] {
        let named = "reports whose living posts were sent advisories";
        assert_eq!(unsupported(unkept.advised()), naming(named));
        assert_eq!(
            unsupported(unkept.put_advised(&herald_core::AdvisedReports::default(), None)),
            naming(named)
        );
    }
}

/// A pull request kept as read at the epoch.
pub(crate) fn kept_pull() -> herald_core::KeptPull {
    let pull = herald_core::Pull {
        project: "p".to_owned(),
        repository: "o/r".to_owned(),
        visibility: herald_core::Visibility::Public,
        number: 1,
        title: "t".to_owned(),
        url: "https://github.com/o/r/pull/1".to_owned(),
        author: None,
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: Timestamp::UNIX_EPOCH,
        updated: Timestamp::UNIX_EPOCH,
        review: herald_core::Review::Unasked,
        reviewers: Vec::new(),
        checks: herald_core::Checks::Absent,
        mergeable: herald_core::Mergeable::Unknown,
        queued: false,
        forge: None,
        head: None,
        unread: false,
        base: None,
        files: None,
        areas: Vec::new(),
        flips: Vec::new(),
    };
    let key = herald_core::PullKey {
        repository: herald_core::RepositoryKey::of(&pull.url, "1").unwrap(),
        number: 1,
    };
    herald_core::KeptPull::read(None, key, pull, Timestamp::UNIX_EPOCH)
}

#[test]
fn the_record_of_each_re_run_kept_nowhere_answers_unsupported_naming_it() {
    let key = herald_core::RunKey {
        api: "https://api.github.com".to_owned(),
        repository: "o/r".to_owned(),
        run: "7".to_owned(),
        job: "70".to_owned(),
        head: "abc".to_owned(),
    };
    let action = herald_core::RerunRecord {
        key: key.clone(),
        operator: "octocat".to_owned(),
        credential: "HERALD_RERUN".to_owned(),
        at: Timestamp::UNIX_EPOCH,
        reruns: 0,
        outcome: herald_core::Reran::Calling,
    };
    for unkept in [None, Some(&Unkept)] {
        assert_eq!(unsupported(unkept.action(&key)), naming("re-runs"));
        assert_eq!(
            unsupported(unkept.put_action(&action, None)),
            naming("re-runs")
        );
    }
}
