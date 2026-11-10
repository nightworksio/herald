use herald_core::{
    Chore, Cursor, DestinationName, Entry, Facts, Identity, Key, Kind, Latest, RouteId, Schedule,
    Shown, SourceName, State, Step, Subscription,
};
use std::fmt;

use herald_ports::{
    Admitted, Announcements, Cursors, Deliveries, Inboxes, Keeping, Keyring, KnownKey, Ledger,
    Notes, Pins, Schedules, Store, StoreError, Subscriptions, Version, Written,
};
use jiff::Timestamp;

mod kv;
mod raced;
mod sql;
mod unkept;

pub use kv::kv_conformance;
pub use sql::sql_conformance;

/// A scheme as its crate writes it, for these tests.
const GITHUB: herald_core::Scheme = herald_core::Scheme {
    name: "github",
    signed: herald_core::Signed::Body {
        headers: &["X-Hub-Signature-256"],
        prefix: "sha256=",
    },
    key: herald_core::KeyForm::Bytes,
};

/// A scheme as its crate writes it, for these tests.
const GITLAB: herald_core::Scheme = herald_core::Scheme {
    name: "gitlab",
    signed: herald_core::Signed::StandardWebhooks {
        id: "webhook-id",
        timestamp: "webhook-timestamp",
        signature: "webhook-signature",
    },
    key: herald_core::KeyForm::Whsec,
};

/// A scheme as its crate writes it, for these tests.
const FORGEJO: herald_core::Scheme = herald_core::Scheme {
    name: "forgejo",
    signed: herald_core::Signed::Body {
        headers: &["X-Forgejo-Signature", "X-Gitea-Signature"],
        prefix: "",
    },
    key: herald_core::KeyForm::Bytes,
};

/// Whether `store`, empty, skips no record, keeps every kind of record it
/// answers as a store must (ADR-0005), and refuses every method of each
/// kind it keeps none of as unsupported. Where it keeps a kind with
/// compare-and-set, as its ledger, cursors and what its projects last
/// announced are, of writers racing for one record exactly one writes, and
/// of runs admitting one delivery at once exactly one admits it fresh.
/// Every store passes this suite, and so does [`crate::FakeStore`].
///
/// # Errors
///
/// The first way the store fails, as a sentence.
pub async fn store_conformance<S: Store>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    unkept::unkept(store, &mut checks).await;
    checks.done()?;
    if !store.skipped().is_empty() {
        return Err(Failure("a store holding no record skipped one".to_owned()));
    }
    if let Some(kept) = store.ledger() {
        match kept.keeping() {
            Keeping::Derived => derived_ledger(kept).await?,
            Keeping::Kept => ledger(kept).await?,
        }
    }
    if let Some(kept) = store.cursors() {
        cursors(kept).await?;
    }
    if let Some(kept) = store.schedules() {
        match kept.keeping() {
            Keeping::Derived => derived_schedules(kept).await?,
            Keeping::Kept => schedules(kept).await?,
        }
    }
    if let Some(kept) = store.subscriptions() {
        subscriptions(kept).await?;
    }
    if let Some(kept) = store.inboxes() {
        inboxes(kept).await?;
    }
    if let Some(kept) = store.pins() {
        pins(kept).await?;
    }
    if let Some(kept) = store.deliveries() {
        deliveries(kept).await?;
    }
    if let Some(kept) = store.notes() {
        notes(kept).await?;
        outboxes(kept).await?;
    }
    if let Some(kept) = store.announcements() {
        latest(kept).await?;
    }
    Ok(())
}

/// Whether `keyring`, empty, keeps each key by its id, the one known last,
/// and forgets only what was read before it is asked to. Every keyring
/// passes this suite.
///
/// # Errors
///
/// The first way the keyring fails, as a sentence.
pub async fn keyring_conformance<K: Keyring>(keyring: &K) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let later = NOON + jiff::SignedDuration::from_hours(1);
    let main = "https://social.example/users/ana#main-key";
    let known = |pem: &str, fetched| KnownKey {
        owner: "https://social.example/users/ana".to_owned(),
        pem: pem.to_owned(),
        fetched,
    };
    checks.ensure(
        keyring.known(main).await?.is_none(),
        "a new keyring knows a key",
    );
    keyring.know(main, &known("first", NOON)).await?;
    keyring.know(main, &known("second", NOON)).await?;
    keyring
        .know("https://other.example/users/bo#key", &known("other", later))
        .await?;
    checks.ensure(
        keyring.known(main).await? == Some(known("second", NOON)),
        "a key's id did not name the key known last",
    );
    keyring.forget_before(later).await?;
    checks.ensure(
        keyring.known(main).await?.is_none(),
        "a key read before the expiry was still known",
    );
    checks.ensure(
        keyring
            .known("https://other.example/users/bo#key")
            .await?
            .is_some(),
        "a key read at the expiry was forgotten",
    );
    checks.done()
}

/// The first way a store fails the conformance suite, as a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure(pub String);

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<StoreError> for Failure {
    fn from(error: StoreError) -> Self {
        Self(format!("the store failed: {error}"))
    }
}

#[derive(Default)]
struct Checks(Vec<String>);

impl Checks {
    fn ensure(&mut self, holds: bool, failure: &str) {
        if !holds {
            self.0.push(failure.to_owned());
        }
    }

    fn written(&mut self, written: Written) -> Version {
        if let Written::Written(version) = written {
            return version;
        }
        self.0
            .push("a write the store should have taken conflicted".to_owned());
        Version(0)
    }

    fn done(self) -> Result<(), Failure> {
        self.0
            .into_iter()
            .next()
            .map_or(Ok(()), |failure| Err(Failure(failure)))
    }
}

fn key(identity: &str) -> Key {
    Key::derive(&RouteId::new("conformance"), &Identity::new(identity))
}

const NOON: Timestamp = Timestamp::constant(1_793_448_000, 0);

fn subscription(destination: &str, project: &str, subscriber: &str) -> Subscription {
    Subscription {
        destination: DestinationName::new(destination),
        project: project.to_owned(),
        subscriber: subscriber.to_owned(),
        inbox: None,
        pending: None,
    }
}

async fn ledger<S: Ledger>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let first = Entry::new(key("first")).attempted(Step::Post, NOON);
    checks.ensure(
        store.entry(&first.key).await?.is_none(),
        "an entry never written was found",
    );
    let v1 = checks.written(store.put_entry(&first, None).await?);
    let read = store.entry(&first.key).await?;
    checks.ensure(
        read.as_ref().map(|record| &record.value) == Some(&first),
        "an entry written was not read back as written",
    );
    checks.ensure(
        read.map(|record| record.version) == Some(v1),
        "an entry was read back at another version than it was written at",
    );
    let again = store.put_entry(&first, None).await?;
    checks.ensure(
        again == Written::Conflict,
        "an entry was written as new over one that exists",
    );
    let landed = first.clone().landed(Step::Post, Facts::default());
    let v2 = checks.written(store.put_entry(&landed, Some(v1)).await?);
    checks.ensure(v2 != v1, "an entry rewritten kept its version");
    let stale = first.clone().landed(Step::Pin, Facts::default());
    let lost = store.put_entry(&stale, Some(v1)).await?;
    checks.ensure(
        lost == Written::Conflict,
        "an entry was written over a version it did not expect",
    );
    let read = store.entry(&first.key).await?;
    checks.ensure(
        read.map(|record| record.value) == Some(landed),
        "a conflicting write changed the entry",
    );
    let other = store.entry(&key("other")).await?;
    checks.ensure(other.is_none(), "writing one entry made another appear");
    expired_entries(store, &mut checks, &first.key).await?;
    raced::entries(store, &mut checks).await?;
    checks.done()
}

/// Whether expiring forgets each entry no run claimed since, keeps the rest
/// and answers the key of each it forgot.
async fn expired_entries<S: Ledger>(
    store: &S,
    checks: &mut Checks,
    unclaimed: &Key,
) -> Result<(), Failure> {
    let later = NOON + jiff::SignedDuration::from_hours(1);
    let old = Entry::new(key("old")).claimed_by("run", NOON, later);
    let new = Entry::new(key("new")).claimed_by("run", later, later);
    checks.written(store.put_entry(&old, None).await?);
    checks.written(store.put_entry(&new, None).await?);
    let expired = store.expire_entries(later).await?;
    checks.ensure(
        store.entry(&old.key).await?.is_none(),
        "an entry no run has claimed since the expiry was kept",
    );
    checks.ensure(
        store.entry(&new.key).await?.is_some(),
        "an entry claimed at the expiry was forgotten",
    );
    checks.ensure(
        store.entry(unclaimed).await?.is_some(),
        "an entry no run ever claimed was forgotten",
    );
    checks.ensure(
        expired == [old.key],
        "expiring entries did not answer the key of each entry it forgot",
    );
    Ok(())
}

async fn derived_ledger<S: Ledger>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let first = Entry::new(key("first")).attempted(Step::Post, NOON);
    checks.ensure(
        store.entry(&first.key).await?.is_none(),
        "an entry no post carries was found",
    );
    let v1 = checks.written(store.put_entry(&first, None).await?);
    let again = store.put_entry(&first, Some(v1)).await?;
    checks.ensure(
        matches!(again, Written::Written(v2) if v2 > v1),
        "a write to a derived ledger was not taken at a later version",
    );
    checks.ensure(
        store.entry(&first.key).await?.is_none(),
        "a derived ledger kept a write",
    );
    checks.done()
}

async fn derived_schedules<S: Schedules>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let schedule = Schedule {
        key: key("1"),
        route: RouteId::new("conformance"),
        thread: "1".to_owned(),
        chore: Chore::Archive,
        due: NOON,
    };
    checks.ensure(
        store.every_schedule().await?.is_empty(),
        "schedules no post carries were found",
    );
    store
        .put_schedules(std::slice::from_ref(&schedule), &[])
        .await?;
    store
        .put_schedules(&[], std::slice::from_ref(&schedule))
        .await?;
    store
        .put_schedules(std::slice::from_ref(&schedule), &[])
        .await?;
    checks.ensure(
        store.every_schedule().await?.is_empty(),
        "a store deriving schedules kept one written",
    );
    checks.done()
}

async fn cursors<S: Cursors>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let source = SourceName::new("feed");
    let cursor = Cursor {
        seen: vec![Identity::new("a")],
        etag: Some("e1".to_owned()),
        last_modified: None,
    };
    checks.ensure(
        store.cursor(&source).await?.is_none(),
        "a cursor never written was found",
    );
    let v1 = checks.written(store.put_cursor(&source, &cursor, None).await?);
    let read = store.cursor(&source).await?;
    checks.ensure(
        read.as_ref().map(|record| &record.value) == Some(&cursor),
        "a cursor written was not read back as written",
    );
    checks.ensure(
        read.map(|record| record.version) == Some(v1),
        "a cursor was read back at another version than it was written at",
    );
    let advanced = Cursor {
        seen: vec![Identity::new("b"), Identity::new("a")],
        etag: Some("e2".to_owned()),
        last_modified: None,
    };
    let v2 = checks.written(store.put_cursor(&source, &advanced, Some(v1)).await?);
    checks.ensure(v2 != v1, "a cursor rewritten kept its version");
    let lost = store.put_cursor(&source, &cursor, Some(v1)).await?;
    checks.ensure(
        lost == Written::Conflict,
        "a cursor was written over a version it did not expect",
    );
    let created = store.put_cursor(&source, &cursor, None).await?;
    checks.ensure(
        created == Written::Conflict,
        "a cursor was written as new over one that exists",
    );
    let read = store.cursor(&source).await?;
    checks.ensure(
        read.map(|record| record.value) == Some(advanced),
        "a conflicting write changed the cursor",
    );
    let other = store.cursor(&SourceName::new("other")).await?;
    checks.ensure(other.is_none(), "writing one cursor made another appear");
    raced::cursors(store, &mut checks).await?;
    checks.done()
}

async fn schedules<S: Schedules>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let schedule = |thread: &str, chore| Schedule {
        key: key(thread),
        route: RouteId::new("conformance"),
        thread: thread.to_owned(),
        chore,
        due: NOON,
    };
    let archive = schedule("1", Chore::Archive);
    let lock = schedule("2", Chore::Lock);
    checks.ensure(
        store.every_schedule().await?.is_empty(),
        "a new store holds schedules",
    );
    store
        .put_schedules(&[archive.clone(), lock.clone()], &[])
        .await?;
    store
        .put_schedules(std::slice::from_ref(&archive), &[])
        .await?;
    let mut held = store.every_schedule().await?;
    held.sort_by(|left, right| left.thread.cmp(&right.thread));
    checks.ensure(
        held == [archive.clone(), lock.clone()],
        "schedules added were not held once each",
    );
    store
        .put_schedules(&[], std::slice::from_ref(&archive))
        .await?;
    store
        .put_schedules(&[], std::slice::from_ref(&archive))
        .await?;
    let held = store.every_schedule().await?;
    checks.ensure(
        held == [lock],
        "a schedule removed was still held, or another went with it",
    );
    checks.done()
}

async fn subscriptions<S: Subscriptions>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let slack = DestinationName::new("slack");
    let alice = subscription("slack", "herald", "alice");
    let bob = subscription("slack", "herald", "bob");
    let elsewhere = [
        subscription("slack", "other", "carol"),
        subscription("matrix", "herald", "dave"),
    ];
    checks.ensure(
        store.subscribers(&slack, "herald").await?.is_empty(),
        "a new store holds subscribers",
    );
    for subscribed in [&alice, &bob, &alice, &elsewhere[0], &elsewhere[1]] {
        store.subscribe(subscribed).await?;
    }
    let mut held = store.subscribers(&slack, "herald").await?;
    held.sort();
    checks.ensure(
        held == [alice.clone(), bob.clone()],
        "the subscribers of a project on a destination were not those subscribed, once each",
    );
    store.unsubscribe(&alice).await?;
    store.unsubscribe(&alice).await?;
    let held = store.subscribers(&slack, "herald").await?;
    checks.ensure(
        held == [bob],
        "a subscriber removed was still held, or another went with them",
    );
    let fediverse = DestinationName::new("fediverse");
    let follower = Subscription {
        inbox: Some("https://social.example/inbox".to_owned()),
        ..subscription("fediverse", "herald", "https://social.example/users/ana")
    };
    store.subscribe(&follower).await?;
    let inboxes: Vec<Option<String>> = store
        .subscribers(&fediverse, "herald")
        .await?
        .into_iter()
        .map(|held| held.inbox)
        .collect();
    checks.ensure(
        inboxes == [Some("https://social.example/inbox".to_owned())],
        "a follower's inbox was not kept with them",
    );
    checks.ensure(
        store.subscribers(&fediverse, "herald").await? == [follower.clone()],
        "a follower's inbox was not kept with its subscription",
    );
    let waiting = Subscription {
        pending: Some("https://social.example/f/1".to_owned()),
        inbox: Some("https://social.example/users/ana/inbox".to_owned()),
        ..follower.clone()
    };
    store.subscribe(&waiting).await?;
    let replaced = store.subscribers(&fediverse, "herald").await? == [waiting.clone()];
    store.subscribe(&follower).await?;
    checks.ensure(
        replaced && store.subscribers(&fediverse, "herald").await? == [follower.clone()],
        "a subscriber subscribed again was not kept once, as subscribed last",
    );
    store.subscribe(&waiting).await?;
    store.unsubscribe(&follower).await?;
    checks.ensure(
        store.subscribers(&fediverse, "herald").await?.is_empty(),
        "a subscriber kept as subscribed otherwise was not forgotten",
    );
    checks.done()
}

async fn inboxes<S: Inboxes>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let published = key("delivered");
    let inbox = "https://social.example/inbox";
    let landed = State::Landed {
        facts: Facts::default(),
    };
    checks.ensure(
        store.delivery(&published, inbox).await?.is_none(),
        "a new store holds a delivery to an inbox",
    );
    store
        .put_delivery(&published, inbox, &State::Attempted { at: NOON })
        .await?;
    store.put_delivery(&published, inbox, &landed).await?;
    store
        .put_delivery(
            &published,
            "https://other.example/inbox",
            &State::Attempted { at: NOON },
        )
        .await?;
    store
        .put_delivery(&key("other"), inbox, &State::Attempted { at: NOON })
        .await?;
    checks.ensure(
        store.delivery(&published, inbox).await? == Some(landed),
        "a delivery to an inbox was not the state written last",
    );
    store.forget_deliveries(&published).await?;
    let mut kept = Vec::new();
    for inbox in [inbox, "https://other.example/inbox"] {
        if store.delivery(&published, inbox).await?.is_some() {
            kept.push(inbox);
        }
    }
    checks.ensure(
        kept.is_empty(),
        "a delivery of an announcement whose deliveries were forgotten was kept",
    );
    checks.ensure(
        store.delivery(&key("other"), inbox).await?.is_some(),
        "forgetting one announcement's deliveries forgot another's",
    );
    checks.done()
}

async fn pins<S: Pins>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let releases = RouteId::new("releases");
    checks.ensure(
        store.pinned(&releases).await?.is_none(),
        "a new store holds a pin",
    );
    store.put_pinned(&releases, "1").await?;
    store.put_pinned(&releases, "2").await?;
    store.put_pinned(&RouteId::new("other"), "3").await?;
    checks.ensure(
        store.pinned(&releases).await?.as_deref() == Some("2"),
        "a route's pin was not the one written last",
    );
    checks.done()
}

async fn deliveries<S: Deliveries>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let later = NOON + jiff::SignedDuration::from_hours(1);
    checks.ensure(
        store.admit(GITHUB, "one", NOON).await? == Admitted::Fresh,
        "a delivery arriving the first time was not fresh",
    );
    checks.ensure(
        store.admit(GITHUB, "one", later).await? == Admitted::Seen,
        "a delivery admitted before was fresh again",
    );
    checks.ensure(
        store.admit(GITLAB, "one", NOON).await? == Admitted::Fresh,
        "another scheme's delivery of the same id was seen",
    );
    store.release(GITHUB, "one").await?;
    checks.ensure(
        store.admit(GITHUB, "one", later).await? == Admitted::Fresh,
        "a released delivery was not admitted again",
    );
    store.admit(FORGEJO, "old", NOON).await?;
    store.admit(FORGEJO, "new", later).await?;
    store.expire_deliveries(later).await?;
    checks.ensure(
        store.admit(FORGEJO, "old", later).await? == Admitted::Fresh,
        "a delivery admitted before the expiry was still seen",
    );
    checks.ensure(
        store.admit(FORGEJO, "new", later).await? == Admitted::Seen,
        "a delivery admitted at the expiry was forgotten",
    );
    raced::admissions(store, &mut checks).await?;
    checks.done()
}

async fn notes<S: Notes>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let published = key("published");
    checks.ensure(
        store.note(&published).await?.is_none(),
        "a new store holds a note",
    );
    let later = NOON + jiff::SignedDuration::from_hours(1);
    store
        .put_note("herald", &published, "{\"n\":1}", NOON)
        .await?;
    store
        .put_note("herald", &published, "{\"n\":2}", NOON)
        .await?;
    store
        .put_note("herald", &key("other"), "{\"n\":3}", later)
        .await?;
    checks.ensure(
        store.note(&published).await?.as_deref() == Some("{\"n\":2}"),
        "an announcement's note was not the one written last",
    );
    store.expire_notes(later).await?;
    checks.ensure(
        store.note(&published).await?.is_none(),
        "a note published before the expiry was kept",
    );
    checks.ensure(
        store.note(&key("other")).await?.as_deref() == Some("{\"n\":3}"),
        "a note published at the expiry was forgotten",
    );
    checks.done()
}

/// The Note named `name`, as the outbox checks write it.
fn written(name: &str) -> String {
    format!("{{\"n\":\"{name}\"}}")
}

async fn outboxes<S: Notes>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let hour = |hours| NOON + jiff::SignedDuration::from_hours(hours);
    for (name, at) in [("first", 0), ("second", 1), ("third", 2), ("third", 2)] {
        store
            .put_note("announcer", &key(name), &written(name), hour(at))
            .await?;
    }
    store
        .put_note(
            "elsewhere",
            &key("elsewhere"),
            &written("elsewhere"),
            hour(1),
        )
        .await?;
    let newest = store.outbox("announcer", None, 2).await?;
    checks.ensure(
        newest.total == 3 && newest.notes == [written("third"), written("second")],
        "an actor's outbox did not list its newest notes first",
    );
    checks.ensure(
        newest.next.is_some(),
        "an outbox with more notes than a page named no next page",
    );
    let older = store.outbox("announcer", newest.next.as_deref(), 2).await?;
    checks.ensure(
        older.notes == [written("first")] && older.next.is_none(),
        "an outbox's next page did not hold the notes older than the last",
    );
    checks.ensure(
        store.outbox("elsewhere", None, 2).await?.notes == [written("elsewhere")],
        "an actor's outbox listed another actor's notes",
    );
    store
        .put_note("announcer", &key("first"), &written("first"), hour(3))
        .await?;
    let again = store.outbox("announcer", None, 3).await?;
    checks.ensure(
        again.total == 3 && again.notes.first() == Some(&written("first")),
        "a note published again was not listed once, as published last",
    );
    store.expire_notes(hour(2)).await?;
    checks.ensure(
        store.outbox("announcer", None, 3).await?.notes == [written("first"), written("third")],
        "an expired note stayed in its actor's outbox",
    );
    checks.done()
}

fn announced(project: &str, version: &str) -> Latest {
    let shown = Shown {
        kind: Kind::RELEASE,
        title: format!("{project} {version}"),
        body: "Notes".to_owned(),
        links: std::iter::once((
            herald_core::RELEASE_LINK.to_owned(),
            "https://example.org".to_owned(),
        ))
        .collect(),
        attributes: herald_core::versioned(version, false),
        occurred: NOON,
    };
    Latest {
        project: project.to_owned(),
        newest: shown.clone(),
        stable: Some(shown),
        routes: std::iter::once(RouteId::new("deploys")).collect(),
    }
}

async fn latest<S: Announcements>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let project = "@scope/herald";
    checks.ensure(
        store.latest(project).await?.is_none() && store.every_latest().await?.is_empty(),
        "a new store holds what a project last announced",
    );
    let first = announced(project, "1.0.0");
    let v1 = checks.written(store.put_latest(&first, None).await?);
    let read = store.latest(project).await?;
    checks.ensure(
        read.as_ref().map(|record| (&record.value, record.version)) == Some((&first, v1)),
        "what a project last announced was not read back as written, at its version",
    );
    let second = announced(project, "1.1.0");
    let v2 = checks.written(store.put_latest(&second, Some(v1)).await?);
    checks.ensure(
        v2 != v1,
        "what a project last announced kept its version when rewritten",
    );
    checks.ensure(
        store.put_latest(&first, Some(v1)).await? == Written::Conflict,
        "what a project last announced was written over a version it did not expect",
    );
    checks.ensure(
        store.put_latest(&first, None).await? == Written::Conflict,
        "what a project last announced was written as new over what exists",
    );
    let other = announced("other", "0.1.0");
    checks.written(store.put_latest(&other, None).await?);
    let mut every = store.every_latest().await?;
    every.sort_by(|a, b| a.project.cmp(&b.project));
    checks.ensure(
        every == [second, other],
        "every project's latest was not listed as written last",
    );
    raced::latest(store, &mut checks).await?;
    checks.done()
}

#[cfg(test)]
mod tests;
