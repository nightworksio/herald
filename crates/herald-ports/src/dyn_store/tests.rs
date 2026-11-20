#![allow(
    clippy::unused_async_trait_impl,
    reason = "a fake answers at once, and is written as the async trait it fakes"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

use herald_core::{
    AuditDay, DestinationName, Feed, Hold, Identity, KeptToken, Kind, PeerKeys, Pending,
    Publication, PublishedKeys, Shown, Window,
};

use super::*;
use crate::erased::Erased;
use crate::store::Keeper;

const NAMESPACE: &str = "namespace";

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

    async fn listing(&self, _: &SourceName) -> Result<Option<Versioned<Listing>>, StoreError> {
        Err(refused("listing"))
    }

    async fn put_listing(
        &self,
        _: &SourceName,
        _: &Listing,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(refused("put_listing"))
    }

    async fn members(&self, _: &SourceName) -> Result<Vec<Versioned<Membership>>, StoreError> {
        Err(refused("members"))
    }

    async fn member(
        &self,
        _: &SourceName,
        _: &str,
    ) -> Result<Option<Versioned<Membership>>, StoreError> {
        Err(refused("member"))
    }

    async fn put_member(
        &self,
        _: &SourceName,
        _: &Membership,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(refused("put_member"))
    }

    async fn forget_member(&self, _: &SourceName, _: &str) -> Result<(), StoreError> {
        Err(refused("forget_member"))
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

    async fn forget_pinned(&self, _: &RouteId, _: &str) -> Result<(), StoreError> {
        Err(refused("forget_pinned"))
    }
}

impl crate::Pulls for Refusing {
    async fn pull(
        &self,
        _: &herald_core::PullKey,
    ) -> Result<Option<Versioned<herald_core::KeptPull>>, StoreError> {
        Err(refused("pull"))
    }

    async fn pulls_of(
        &self,
        _: &herald_core::RepositoryKey,
    ) -> Result<Vec<Versioned<herald_core::KeptPull>>, StoreError> {
        Err(refused("pulls_of"))
    }

    async fn put_pull(
        &self,
        _: &herald_core::KeptPull,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(refused("put_pull"))
    }

    async fn expire_pulls(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(refused("expire_pulls"))
    }

    async fn reconciled(
        &self,
        _: &SourceName,
    ) -> Result<Option<Versioned<herald_core::Reconciled>>, StoreError> {
        Err(refused("reconciled"))
    }

    async fn put_reconciled(
        &self,
        _: &SourceName,
        _: &herald_core::Reconciled,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(refused("put_reconciled"))
    }
}

impl crate::Decisions for Refusing {
    async fn decision(&self, _: &Key) -> Result<Option<herald_core::Decided>, StoreError> {
        Err(refused("decision"))
    }

    async fn put_decision(&self, _: &herald_core::Decided) -> Result<(), StoreError> {
        Err(refused("put_decision"))
    }

    async fn expire_decisions(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(refused("expire_decisions"))
    }
}

impl Positions for Refusing {
    async fn position(&self, _: &DestinationName) -> Result<Option<String>, StoreError> {
        Err(refused("position"))
    }

    async fn put_position(&self, _: &DestinationName, _: &str) -> Result<(), StoreError> {
        Err(refused("put_position"))
    }
}

impl Tokens for Refusing {
    async fn token(&self, _: &str) -> Result<Option<Versioned<KeptToken>>, StoreError> {
        Err(refused("token"))
    }

    async fn put_token(&self, _: &KeptToken, _: Option<Version>) -> Result<Written, StoreError> {
        Err(refused("put_token"))
    }
}

impl Published for Refusing {
    async fn published_keys(&self) -> Result<Option<Versioned<PublishedKeys>>, StoreError> {
        Err(refused("published_keys"))
    }

    async fn put_published_keys(
        &self,
        _: &PublishedKeys,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(refused("put_published_keys"))
    }

    async fn publication(&self) -> Result<Option<Versioned<Publication>>, StoreError> {
        Err(refused("publication"))
    }

    async fn put_publication(
        &self,
        _: &Publication,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(refused("put_publication"))
    }

    async fn feed(&self, _: &DestinationName) -> Result<Option<Versioned<Feed>>, StoreError> {
        Err(refused("feed"))
    }

    async fn put_feed(
        &self,
        _: &DestinationName,
        _: &Feed,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(refused("put_feed"))
    }

    async fn peer_keys(&self, _: &str) -> Result<Option<Versioned<PeerKeys>>, StoreError> {
        Err(refused("peer_keys"))
    }

    async fn put_peer_keys(
        &self,
        _: &str,
        _: &PeerKeys,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(refused("put_peer_keys"))
    }

    async fn audit(&self, _: &str, _: &str) -> Result<Option<Versioned<AuditDay>>, StoreError> {
        Err(refused("audit"))
    }

    async fn put_audit(&self, _: &AuditDay, _: Option<Version>) -> Result<Written, StoreError> {
        Err(refused("put_audit"))
    }

    async fn audits(&self, _: &str) -> Result<Vec<AuditDay>, StoreError> {
        Err(refused("audits"))
    }

    async fn expire_audits(&self, _: &str) -> Result<(), StoreError> {
        Err(refused("expire_audits"))
    }

    async fn window(&self, _: &str) -> Result<Option<Versioned<Window>>, StoreError> {
        Err(refused("window"))
    }

    async fn put_window(
        &self,
        _: &str,
        _: &Window,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(refused("put_window"))
    }
}

impl Waiting for Refusing {
    async fn pending(&self, _: &Hold) -> Result<Option<Versioned<Pending>>, StoreError> {
        Err(refused("pending"))
    }

    async fn every_pending(&self) -> Result<Vec<Versioned<Pending>>, StoreError> {
        Err(refused("every_pending"))
    }

    async fn put_pending(&self, _: &Pending, _: Option<Version>) -> Result<Written, StoreError> {
        Err(refused("put_pending"))
    }

    async fn drop_pending(&self, _: &Hold) -> Result<(), StoreError> {
        Err(refused("drop_pending"))
    }
}

impl Deliveries for Refusing {
    async fn admit(&self, _: &'static str, _: &str, _: Timestamp) -> Result<Admitted, StoreError> {
        Err(refused("admit"))
    }

    async fn seen(&self, _: &'static str, _: &str) -> Result<bool, StoreError> {
        Err(refused("seen"))
    }

    async fn release(&self, _: &'static str, _: &str) -> Result<(), StoreError> {
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

    async fn forget_note(&self, _: &Key) -> Result<(), StoreError> {
        Err(refused("forget_note"))
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

    async fn forget_latest(&self, _: &str) -> Result<(), StoreError> {
        Err(refused("forget_latest"))
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

    fn writes_one_at_a_time(&self) -> bool {
        self.holding
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
            [holding; 15]
        );
        assert_eq!(Keeper::skipped(&erased), ["corrupt.json"]);
        assert_eq!(Keeper::writes_one_at_a_time(&erased), holding);
    }
    let unheld = Refusing { holding: false };
    let erased = Erased(&unheld as &dyn DynStore);
    assert_eq!(
        answered(erased.pinned(&RouteId::new("r"))),
        Some(StoreError::Unsupported { what: "pins" })
    );
}

#[test]
fn every_answer_of_an_erased_store_on_an_owner_source_is_the_store_s_own() {
    let store = Refusing { holding: true };
    let erased = Erased(&store as &dyn DynStore);
    let source = SourceName::new("s");
    let (listing, membership) = owned(Timestamp::UNIX_EPOCH);
    let refusals = [
        answered(erased.listing(&source)),
        answered(erased.put_listing(&source, &listing, None)),
        answered(erased.members(&source)),
        answered(erased.member(&source, "1")),
        answered(erased.put_member(&source, &membership, None)),
        answered(erased.forget_member(&source, "1")),
    ];
    let by = [
        "listing",
        "put_listing",
        "members",
        "member",
        "put_member",
        "forget_member",
    ];
    assert_eq!(refusals, by.map(|by| Some(refused(by))));
    let kept = KeptToken {
        name: "github_apps.herald/1".to_owned(),
        sealed: "c2VhbGVk".to_owned(),
        expires: Timestamp::UNIX_EPOCH,
    };
    assert_eq!(
        [
            answered(erased.token(&kept.name)),
            answered(erased.put_token(&kept, None)),
        ],
        [Some(refused("token")), Some(refused("put_token"))]
    );
    let unheld = Refusing { holding: false };
    let erased = Erased(&unheld as &dyn DynStore);
    let unkept = Some(StoreError::Unsupported {
        what: "minted tokens",
    });
    assert_eq!(answered(erased.token(&kept.name)), unkept);
    assert_eq!(answered(erased.put_token(&kept, None)), unkept);
}

#[test]
fn an_erased_store_answers_for_what_the_instance_published_as_the_store_it_erases_does() {
    let keys = PublishedKeys {
        sequence: 1,
        keys: vec!["z6Mkkey".to_owned()],
    };
    let (feed, releases) = (Feed::default(), DestinationName::new("releases"));
    let peer = PeerKeys {
        pinned: Vec::new(),
        sequence: 1,
        keys: Vec::new(),
        read: Timestamp::UNIX_EPOCH,
        refresh: 60,
    };
    let audit = AuditDay::of("upstream", Timestamp::UNIX_EPOCH);
    let window = Window {
        start: Timestamp::UNIX_EPOCH,
        count: 1,
        ..Window::default()
    };
    let store = Refusing { holding: true };
    let erased = Erased(&store as &dyn DynStore);
    assert_eq!(
        [
            answered(erased.published_keys()),
            answered(erased.put_published_keys(&keys, None)),
            answered(erased.publication()),
            answered(erased.put_publication(&Publication::default(), None)),
            answered(erased.feed(&releases)),
            answered(erased.put_feed(&releases, &feed, None)),
            answered(erased.peer_keys("upstream")),
            answered(erased.put_peer_keys("upstream", &peer, None)),
            answered(erased.audit("upstream", "2027-01-15")),
            answered(erased.put_audit(&audit, None)),
            answered(erased.audits("upstream")),
            answered(erased.expire_audits("2027-01-15")),
            answered(erased.window("upstream")),
            answered(erased.put_window("upstream", &window, None)),
        ],
        [
            Some(refused("published_keys")),
            Some(refused("put_published_keys")),
            Some(refused("publication")),
            Some(refused("put_publication")),
            Some(refused("feed")),
            Some(refused("put_feed")),
            Some(refused("peer_keys")),
            Some(refused("put_peer_keys")),
            Some(refused("audit")),
            Some(refused("put_audit")),
            Some(refused("audits")),
            Some(refused("expire_audits")),
            Some(refused("window")),
            Some(refused("put_window")),
        ]
    );
    let unheld = Refusing { holding: false };
    let erased = Erased(&unheld as &dyn DynStore);
    let unkept = Some(StoreError::Unsupported {
        what: "published key sets, feeds, and peers' key sets and audits",
    });
    assert_eq!(answered(erased.published_keys()), unkept);
    assert_eq!(answered(erased.put_published_keys(&keys, None)), unkept);
    assert_eq!(answered(erased.publication()), unkept);
    assert_eq!(
        answered(erased.put_publication(&Publication::default(), None)),
        unkept
    );
    assert_eq!(answered(erased.feed(&releases)), unkept);
    assert_eq!(answered(erased.put_feed(&releases, &feed, None)), unkept);
    assert_eq!(answered(erased.peer_keys("upstream")), unkept);
    assert_eq!(
        answered(erased.put_peer_keys("upstream", &peer, None)),
        unkept
    );
    assert_eq!(answered(erased.audit("upstream", "1970-01-01")), unkept);
    assert_eq!(answered(erased.put_audit(&audit, None)), unkept);
    assert_eq!(answered(erased.audits("upstream")), unkept);
    assert_eq!(answered(erased.expire_audits("1970-01-01")), unkept);
    assert_eq!(answered(erased.window("upstream")), unkept);
    assert_eq!(
        answered(erased.put_window("upstream", &window, None)),
        unkept
    );
}

fn owned(at: Timestamp) -> (Listing, Membership) {
    let listing = Listing {
        owner: herald_core::Owner::User,
        pages: Vec::new(),
        first: at,
        listed: at,
        fingerprint: "f".to_owned(),
        file: None,
        events: None,
        stale: false,
    };
    let membership = Membership {
        repository: herald_core::Listed {
            id: "1".to_owned(),
            name: "n".to_owned(),
            path: "o/n".to_owned(),
            topics: Vec::new(),
            properties: BTreeMap::new(),
            visibility: herald_core::Visibility::Public,
            archived: false,
            fork: false,
            template: false,
            language: None,
            linked: None,
        },
        discovered: at,
        left: None,
        cursor: None,
        collides: None,
        hooked: false,
    };
    (listing, membership)
}

/// What a project last announced, as the erased store is asked to keep.
fn announced(at: Timestamp) -> Latest {
    Latest {
        project: "p".to_owned(),
        newest: Shown {
            kind: Kind::RELEASE,
            attributes: BTreeMap::default(),
            title: "t".to_owned(),
            body: String::new(),
            links: BTreeMap::new(),
            occurred: at,
            identity: None,
        },
        stable: None,
        routes: BTreeSet::new(),
    }
}

/// A decision keyed `key`, made at `at`.
fn decided(key: Key, at: Timestamp) -> herald_core::Decided {
    herald_core::Decided {
        key,
        task: "t".to_owned(),
        subject: "s".to_owned(),
        answer: herald_core::Answer {
            model: "m".to_owned(),
            choice: None,
            probabilities: std::collections::BTreeMap::new(),
        },
        at,
    }
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
    let latest = announced(at);
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
        answered(erased.forget_pinned(&route, "post")),
        answered(erased.position(&subscription.destination)),
        answered(erased.put_position(&subscription.destination, "s1")),
        answered(erased.decision(&key)),
        answered(erased.put_decision(&decided(key, at))),
        answered(erased.expire_decisions(at)),
        answered(erased.admit(NAMESPACE, "1", at)),
        answered(erased.seen(NAMESPACE, "1")),
        answered(erased.release(NAMESPACE, "1")),
        answered(erased.expire_deliveries(at)),
        answered(erased.note(&key)),
        answered(erased.put_note("a", &key, "n", at)),
        answered(erased.outbox("a", None, 1)),
        answered(erased.expire_notes(at)),
        answered(erased.forget_note(&key)),
        answered(erased.latest("p")),
        answered(erased.every_latest()),
        answered(erased.put_latest(&latest, None)),
        answered(erased.forget_latest("p")),
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
        "forget_pinned",
        "position",
        "put_position",
        "decision",
        "put_decision",
        "expire_decisions",
        "admit",
        "seen",
        "release",
        "expire_deliveries",
        "note",
        "put_note",
        "outbox",
        "expire_notes",
        "forget_note",
        "latest",
        "every_latest",
        "put_latest",
        "forget_latest",
        "delivery",
        "put_delivery",
        "forget_deliveries",
        "known",
        "know",
        "forget_before",
    ];
    assert_eq!(refusals, by.map(|by| Some(refused(by))));
}

#[test]
fn every_answer_of_an_erased_store_on_pull_requests_is_the_store_s_own() {
    let store = Refusing { holding: true };
    let erased = Erased(&store as &dyn DynStore);
    let kept = crate::store::tests::kept_pull();
    let source = SourceName::new("s");
    let reconciled = herald_core::Reconciled {
        repository: None,
        at: None,
        validators: Vec::new(),
    };
    let refusals = [
        answered(erased.pull(&kept.key)),
        answered(erased.pulls_of(&kept.key.repository)),
        answered(erased.put_pull(&kept, None)),
        answered(erased.expire_pulls(Timestamp::UNIX_EPOCH)),
        answered(erased.reconciled(&source)),
        answered(erased.put_reconciled(&source, &reconciled, None)),
    ];
    let by = [
        "pull",
        "pulls_of",
        "put_pull",
        "expire_pulls",
        "reconciled",
        "put_reconciled",
    ];
    assert_eq!(refusals, by.map(|by| Some(refused(by))));
}

#[test]
fn every_answer_of_an_erased_store_on_held_events_is_the_store_s_own() {
    let store = Refusing { holding: true };
    let erased = Erased(&store as &dyn DynStore);
    let hold = Hold::Tag {
        source: SourceName::new("s"),
        identity: Identity::new("1"),
    };
    let pending = Pending {
        hold: hold.clone(),
        events: Vec::new(),
        opened: Timestamp::UNIX_EPOCH,
        due: Timestamp::UNIX_EPOCH,
        closed: None,
        landed: false,
    };
    let refusals = [
        answered(erased.pending(&hold)),
        answered(erased.every_pending()),
        answered(erased.put_pending(&pending, None)),
        answered(erased.drop_pending(&hold)),
    ];
    let by = ["pending", "every_pending", "put_pending", "drop_pending"];
    assert_eq!(refusals, by.map(|by| Some(refused(by))));
}
