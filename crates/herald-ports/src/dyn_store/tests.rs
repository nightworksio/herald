#![allow(
    clippy::unused_async_trait_impl,
    reason = "a fake answers at once, and is written as the async trait it fakes"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

use herald_core::{Identity, KeyForm, Kind, Shown, Signed};

use super::*;

const SCHEME: Scheme = Scheme {
    name: "scheme",
    signed: Signed::Hashed,
    key: KeyForm::Bytes,
};

struct Refusing {
    holding: bool,
}

fn refused(by: &str) -> StoreError {
    StoreError::Unavailable {
        reason: by.to_owned(),
    }
}

impl Ledger for Refusing {
    fn keeping(&self) -> Keeping {
        Keeping::Derived
    }

    async fn entry(&self, _: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        Err(refused("entry"))
    }

    async fn put_entry(&self, _: &Entry, _: Option<Version>) -> Result<Written, StoreError> {
        Err(refused("put_entry"))
    }

    async fn expire_entries(&self, _: Timestamp) -> Result<Vec<Key>, StoreError> {
        Err(refused("expire_entries"))
    }
}

impl Cursors for Refusing {
    async fn cursor(&self, _: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        Err(refused("cursor"))
    }

    async fn put_cursor(
        &self,
        _: &SourceName,
        _: &Cursor,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(refused("put_cursor"))
    }
}

impl Schedules for Refusing {
    fn keeping(&self) -> Keeping {
        Keeping::Kept
    }

    async fn every_schedule(&self) -> Result<Vec<Schedule>, StoreError> {
        Err(refused("every_schedule"))
    }

    async fn put_schedules(&self, _: &[Schedule], _: &[Schedule]) -> Result<(), StoreError> {
        Err(refused("put_schedules"))
    }
}

impl Subscriptions for Refusing {
    async fn subscribers(
        &self,
        _: &DestinationName,
        _: &str,
    ) -> Result<Vec<Subscription>, StoreError> {
        Err(refused("subscribers"))
    }

    async fn subscribe(&self, _: &Subscription) -> Result<(), StoreError> {
        Err(refused("subscribe"))
    }

    async fn unsubscribe(&self, _: &Subscription) -> Result<(), StoreError> {
        Err(refused("unsubscribe"))
    }
}

impl Pins for Refusing {
    async fn pinned(&self, _: &RouteId) -> Result<Option<String>, StoreError> {
        Err(refused("pinned"))
    }

    async fn put_pinned(&self, _: &RouteId, _: &str) -> Result<(), StoreError> {
        Err(refused("put_pinned"))
    }
}

impl Deliveries for Refusing {
    async fn admit(&self, _: Scheme, _: &str, _: Timestamp) -> Result<Admitted, StoreError> {
        Err(refused("admit"))
    }

    async fn release(&self, _: Scheme, _: &str) -> Result<(), StoreError> {
        Err(refused("release"))
    }

    async fn expire_deliveries(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(refused("expire_deliveries"))
    }
}

impl Notes for Refusing {
    async fn note(&self, _: &Key) -> Result<Option<String>, StoreError> {
        Err(refused("note"))
    }

    async fn put_note(&self, _: &str, _: &Key, _: &str, _: Timestamp) -> Result<(), StoreError> {
        Err(refused("put_note"))
    }

    async fn outbox(&self, _: &str, _: Option<&str>, _: usize) -> Result<Outboxed, StoreError> {
        Err(refused("outbox"))
    }

    async fn expire_notes(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(refused("expire_notes"))
    }
}

impl Announcements for Refusing {
    async fn latest(&self, _: &str) -> Result<Option<Versioned<Latest>>, StoreError> {
        Err(refused("latest"))
    }

    async fn every_latest(&self) -> Result<Vec<Latest>, StoreError> {
        Err(refused("every_latest"))
    }

    async fn put_latest(&self, _: &Latest, _: Option<Version>) -> Result<Written, StoreError> {
        Err(refused("put_latest"))
    }
}

impl Inboxes for Refusing {
    async fn delivery(&self, _: &Key, _: &str) -> Result<Option<State>, StoreError> {
        Err(refused("delivery"))
    }

    async fn put_delivery(&self, _: &Key, _: &str, _: &State) -> Result<(), StoreError> {
        Err(refused("put_delivery"))
    }

    async fn forget_deliveries(&self, _: &Key) -> Result<(), StoreError> {
        Err(refused("forget_deliveries"))
    }
}

impl Keyring for Refusing {
    async fn known(&self, _: &str) -> Result<Option<KnownKey>, StoreError> {
        Err(refused("known"))
    }

    async fn know(&self, _: &str, _: &KnownKey) -> Result<(), StoreError> {
        Err(refused("know"))
    }

    async fn forget_before(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(refused("forget_before"))
    }
}

impl Keeper for Refusing {
    fn holds(&self, _: Held) -> bool {
        self.holding
    }

    fn skipped(&self) -> Vec<String> {
        vec!["corrupt.json".to_owned()]
    }
}

fn answered<T>(answer: impl Future<Output = Result<T, StoreError>>) -> Option<StoreError> {
    pollster::block_on(answer).err()
}

#[test]
fn an_erased_store_holds_and_skips_what_the_store_it_erases_does() {
    for holding in [true, false] {
        let store = Refusing { holding };
        let erased = Erased(&store as &dyn DynStore);
        assert_eq!(
            Held::ALL.map(|held| Keeper::holds(&erased, held)),
            [holding; 9]
        );
        assert_eq!(Keeper::skipped(&erased), ["corrupt.json"]);
    }
    let unheld = Refusing { holding: false };
    let erased = Erased(&unheld as &dyn DynStore);
    assert_eq!(
        answered(erased.pinned(&RouteId::new("r"))),
        Some(StoreError::Unsupported { what: "pins" })
    );
}

#[test]
fn every_answer_of_an_erased_store_is_the_store_s_own() {
    let store = Refusing { holding: true };
    let erased = Erased(&store as &dyn DynStore);
    let key = Key::derive(&RouteId::new("r"), &Identity::new("1"));
    let at = Timestamp::UNIX_EPOCH;
    let source = SourceName::new("s");
    let route = RouteId::new("r");
    let subscription = Subscription {
        destination: DestinationName::new("d"),
        project: "p".to_owned(),
        subscriber: "u".to_owned(),
        inbox: None,
        pending: None,
    };
    let known = KnownKey {
        owner: "o".to_owned(),
        pem: "k".to_owned(),
        fetched: at,
    };
    let latest = Latest {
        project: "p".to_owned(),
        newest: Shown {
            kind: Kind::RELEASE,
            attributes: BTreeMap::default(),
            title: "t".to_owned(),
            body: String::new(),
            links: BTreeMap::new(),
            occurred: at,
        },
        stable: None,
        routes: BTreeSet::new(),
    };
    assert_eq!(Ledger::keeping(&erased), Keeping::Derived);
    assert_eq!(Schedules::keeping(&erased), Keeping::Kept);
    let refusals = [
        answered(erased.entry(&key)),
        answered(erased.put_entry(&Entry::new(key), None)),
        answered(erased.expire_entries(at)),
        answered(erased.cursor(&source)),
        answered(erased.put_cursor(&source, &Cursor::default(), None)),
        answered(erased.every_schedule()),
        answered(erased.put_schedules(&[], &[])),
        answered(erased.subscribers(&subscription.destination, "p")),
        answered(erased.subscribe(&subscription)),
        answered(erased.unsubscribe(&subscription)),
        answered(erased.pinned(&route)),
        answered(erased.put_pinned(&route, "post")),
        answered(erased.admit(SCHEME, "1", at)),
        answered(erased.release(SCHEME, "1")),
        answered(erased.expire_deliveries(at)),
        answered(erased.note(&key)),
        answered(erased.put_note("a", &key, "n", at)),
        answered(erased.outbox("a", None, 1)),
        answered(erased.expire_notes(at)),
        answered(erased.latest("p")),
        answered(erased.every_latest()),
        answered(erased.put_latest(&latest, None)),
        answered(erased.delivery(&key, "i")),
        answered(erased.put_delivery(&key, "i", &State::Attempted { at })),
        answered(erased.forget_deliveries(&key)),
        answered(erased.known("id")),
        answered(erased.know("id", &known)),
        answered(erased.forget_before(at)),
    ];
    let by = [
        "entry",
        "put_entry",
        "expire_entries",
        "cursor",
        "put_cursor",
        "every_schedule",
        "put_schedules",
        "subscribers",
        "subscribe",
        "unsubscribe",
        "pinned",
        "put_pinned",
        "admit",
        "release",
        "expire_deliveries",
        "note",
        "put_note",
        "outbox",
        "expire_notes",
        "latest",
        "every_latest",
        "put_latest",
        "delivery",
        "put_delivery",
        "forget_deliveries",
        "known",
        "know",
        "forget_before",
    ];
    assert_eq!(refusals, by.map(|by| Some(refused(by))));
}
