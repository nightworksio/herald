use herald_core::{
    Chore, Cursor, DestinationName, Entry, Facts, Identity, Key, Kind, Latest, RouteId, Schedule,
    Scheme, Shown, SourceName, Step, Subscription,
};
use std::fmt;

use herald_ports::{Admitted, Keeping, Keyring, KnownKey, Store, StoreError, Version, Written};
use jiff::Timestamp;

/// Whether `store`, empty, keeps every record its capabilities declare as a
/// store must (ADR-0005), and refuses as unsupported every kind it does not
/// declare. Every store passes this suite, and so does [`crate::FakeStore`].
///
/// # Errors
///
/// The first way the store fails, as a sentence.
pub async fn store_conformance<S: Store>(store: &S) -> Result<(), Failure> {
    let capabilities = store.capabilities();
    match capabilities.ledger {
        Keeping::Kept => ledger(store).await?,
        Keeping::Derived => derived_ledger(store).await?,
        Keeping::None => unsupported(&store.entry(&key("absent")).await.map(|_| ()), "ledger")?,
    }
    if capabilities.cursors {
        cursors(store).await?;
    } else {
        unsupported(
            &store.cursor(&SourceName::new("absent")).await.map(|_| ()),
            "cursors",
        )?;
    }
    match capabilities.schedules {
        Keeping::Kept => schedules(store).await?,
        Keeping::Derived => derived_schedules(store).await?,
        Keeping::None => unsupported(&store.schedules().await.map(|_| ()), "schedules")?,
    }
    if capabilities.subscriptions {
        subscriptions(store).await?;
    } else {
        unsupported(
            &store
                .subscribe(&subscription("discord", "project", "user"))
                .await,
            "subscriptions",
        )?;
    }
    if capabilities.pins {
        pins(store).await?;
    } else {
        unsupported(
            &store.pinned(&RouteId::new("absent")).await.map(|_| ()),
            "pins",
        )?;
    }
    if capabilities.deliveries {
        deliveries(store).await?;
    } else {
        unsupported(
            &store
                .admit(Scheme::GitHub, "absent", NOON)
                .await
                .map(|_| ()),
            "deliveries",
        )?;
    }
    if capabilities.notes {
        notes(store).await?;
    } else {
        unsupported(&store.note(&key("absent")).await.map(|_| ()), "notes")?;
    }
    if capabilities.latest {
        latest(store).await?;
    } else {
        unsupported(&store.latest("absent").await.map(|_| ()), "latest")?;
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

fn unsupported(answer: &Result<(), StoreError>, what: &str) -> Result<(), Failure> {
    let mut checks = Checks::default();
    checks.ensure(
        matches!(answer, Err(StoreError::Unsupported { .. })),
        &format!("a store that does not declare {what} must answer unsupported"),
    );
    checks.done()
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
    }
}

async fn ledger<S: Store>(store: &S) -> Result<(), Failure> {
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
    checks.done()
}

async fn derived_ledger<S: Store>(store: &S) -> Result<(), Failure> {
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

async fn derived_schedules<S: Store>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let schedule = Schedule {
        key: key("1"),
        route: RouteId::new("conformance"),
        thread: "1".to_owned(),
        chore: Chore::Archive,
        due: NOON,
    };
    checks.ensure(
        store.schedules().await?.is_empty(),
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
        store.schedules().await?.is_empty(),
        "a store deriving schedules kept one written",
    );
    checks.done()
}

async fn cursors<S: Store>(store: &S) -> Result<(), Failure> {
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
    checks.done()
}

async fn schedules<S: Store>(store: &S) -> Result<(), Failure> {
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
        store.schedules().await?.is_empty(),
        "a new store holds schedules",
    );
    store
        .put_schedules(&[archive.clone(), lock.clone()], &[])
        .await?;
    store
        .put_schedules(std::slice::from_ref(&archive), &[])
        .await?;
    let mut held = store.schedules().await?;
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
    let held = store.schedules().await?;
    checks.ensure(
        held == [lock],
        "a schedule removed was still held, or another went with it",
    );
    checks.done()
}

async fn subscriptions<S: Store>(store: &S) -> Result<(), Failure> {
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
        store.subscribers(&fediverse, "herald").await? == [follower],
        "a follower's inbox was not kept with its subscription",
    );
    checks.done()
}

async fn pins<S: Store>(store: &S) -> Result<(), Failure> {
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

async fn deliveries<S: Store>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let later = NOON + jiff::SignedDuration::from_hours(1);
    checks.ensure(
        store.admit(Scheme::GitHub, "one", NOON).await? == Admitted::Fresh,
        "a delivery arriving the first time was not fresh",
    );
    checks.ensure(
        store.admit(Scheme::GitHub, "one", later).await? == Admitted::Seen,
        "a delivery admitted before was fresh again",
    );
    checks.ensure(
        store.admit(Scheme::GitLab, "one", NOON).await? == Admitted::Fresh,
        "another scheme's delivery of the same id was seen",
    );
    store.release(Scheme::GitHub, "one").await?;
    checks.ensure(
        store.admit(Scheme::GitHub, "one", later).await? == Admitted::Fresh,
        "a released delivery was not admitted again",
    );
    store.admit(Scheme::Forgejo, "old", NOON).await?;
    store.admit(Scheme::Forgejo, "new", later).await?;
    store.expire_deliveries(later).await?;
    checks.ensure(
        store.admit(Scheme::Forgejo, "old", later).await? == Admitted::Fresh,
        "a delivery admitted before the expiry was still seen",
    );
    checks.ensure(
        store.admit(Scheme::Forgejo, "new", later).await? == Admitted::Seen,
        "a delivery admitted at the expiry was forgotten",
    );
    checks.done()
}

async fn notes<S: Store>(store: &S) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let published = key("published");
    checks.ensure(
        store.note(&published).await?.is_none(),
        "a new store holds a note",
    );
    store.put_note(&published, "{\"n\":1}").await?;
    store.put_note(&published, "{\"n\":2}").await?;
    store.put_note(&key("other"), "{\"n\":3}").await?;
    checks.ensure(
        store.note(&published).await?.as_deref() == Some("{\"n\":2}"),
        "an announcement's note was not the one written last",
    );
    checks.done()
}

fn announced(project: &str, version: &str) -> Latest {
    let shown = Shown {
        kind: Kind::Release,
        version: Some(version.to_owned()),
        title: format!("{project} {version}"),
        body: "Notes".to_owned(),
        links: std::iter::once(("release".to_owned(), "https://example.org".to_owned())).collect(),
        prerelease: false,
        occurred: NOON,
    };
    Latest {
        project: project.to_owned(),
        newest: shown.clone(),
        stable: Some(shown),
        routes: std::iter::once(RouteId::new("deploys")).collect(),
    }
}

async fn latest<S: Store>(store: &S) -> Result<(), Failure> {
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
    checks.done()
}

#[cfg(test)]
mod tests;
