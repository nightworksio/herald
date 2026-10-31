use herald_core::{
    Chore, Cursor, DestinationName, Entry, Facts, Identity, Key, RouteId, Schedule, SourceName,
    Step, Subscription,
};
use herald_ports::{Store, StoreError, Version, Written};
use jiff::Timestamp;

/// Whether `store`, empty, keeps every record its capabilities declare as a
/// store must (ADR-0005), and refuses as unsupported every kind it does not
/// declare. Every store passes this suite, and so does [`crate::FakeStore`].
///
/// # Errors
///
/// The first way the store fails, as a sentence.
pub async fn store_conformance<S: Store>(store: &S) -> Result<(), String> {
    let capabilities = store.capabilities();
    if capabilities.ledger {
        ledger(store).await?;
    } else {
        unsupported(&store.entry(&key("absent")).await.map(|_| ()), "ledger")?;
    }
    if capabilities.cursors {
        cursors(store).await?;
    } else {
        unsupported(
            &store.cursor(&SourceName::new("absent")).await.map(|_| ()),
            "cursors",
        )?;
    }
    if capabilities.schedules {
        schedules(store).await?;
    } else {
        unsupported(&store.schedules().await.map(|_| ()), "schedules")?;
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
    Ok(())
}

fn ensure(holds: bool, failure: &str) -> Result<(), String> {
    if holds {
        Ok(())
    } else {
        Err(failure.to_owned())
    }
}

fn failed(error: &StoreError) -> String {
    format!("the store failed: {error}")
}

fn unsupported(answer: &Result<(), StoreError>, what: &str) -> Result<(), String> {
    ensure(
        matches!(answer, Err(StoreError::Unsupported { .. })),
        &format!("a store that does not declare {what} must answer unsupported"),
    )
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
    }
}

fn version(written: Written) -> Result<Version, String> {
    match written {
        Written::Written(version) => Ok(version),
        Written::Conflict => Err("a write the store should have taken conflicted".to_owned()),
    }
}

async fn ledger<S: Store>(store: &S) -> Result<(), String> {
    let first = Entry::new(key("first")).attempted(Step::Post, NOON);
    ensure(
        store
            .entry(&first.key)
            .await
            .map_err(|error| failed(&error))?
            .is_none(),
        "an entry never written was found",
    )?;
    let v1 = version(
        store
            .put_entry(&first, None)
            .await
            .map_err(|error| failed(&error))?,
    )?;
    let read = store
        .entry(&first.key)
        .await
        .map_err(|error| failed(&error))?;
    ensure(
        read.as_ref().map(|record| &record.value) == Some(&first),
        "an entry written was not read back as written",
    )?;
    ensure(
        read.map(|record| record.version) == Some(v1),
        "an entry was read back at another version than it was written at",
    )?;
    let again = store
        .put_entry(&first, None)
        .await
        .map_err(|error| failed(&error))?;
    ensure(
        again == Written::Conflict,
        "an entry was written as new over one that exists",
    )?;
    let landed = first.clone().landed(
        Step::Post,
        Facts {
            message: Some("1".to_owned()),
            ..Facts::default()
        },
    );
    let v2 = version(
        store
            .put_entry(&landed, Some(v1))
            .await
            .map_err(|error| failed(&error))?,
    )?;
    ensure(v2 != v1, "an entry rewritten kept its version")?;
    let stale = first.clone().landed(Step::Pin, Facts::default());
    let lost = store
        .put_entry(&stale, Some(v1))
        .await
        .map_err(|error| failed(&error))?;
    ensure(
        lost == Written::Conflict,
        "an entry was written over a version it did not expect",
    )?;
    let read = store
        .entry(&first.key)
        .await
        .map_err(|error| failed(&error))?;
    ensure(
        read.map(|record| record.value) == Some(landed),
        "a conflicting write changed the entry",
    )?;
    let other = store
        .entry(&key("other"))
        .await
        .map_err(|error| failed(&error))?;
    ensure(other.is_none(), "writing one entry made another appear")?;
    Ok(())
}

async fn cursors<S: Store>(store: &S) -> Result<(), String> {
    let source = SourceName::new("feed");
    let cursor = Cursor {
        seen: vec![Identity::new("a")],
        etag: Some("e1".to_owned()),
        last_modified: None,
    };
    ensure(
        store
            .cursor(&source)
            .await
            .map_err(|error| failed(&error))?
            .is_none(),
        "a cursor never written was found",
    )?;
    let v1 = version(
        store
            .put_cursor(&source, &cursor, None)
            .await
            .map_err(|error| failed(&error))?,
    )?;
    let read = store
        .cursor(&source)
        .await
        .map_err(|error| failed(&error))?;
    ensure(
        read.as_ref().map(|record| &record.value) == Some(&cursor),
        "a cursor written was not read back as written",
    )?;
    ensure(
        read.map(|record| record.version) == Some(v1),
        "a cursor was read back at another version than it was written at",
    )?;
    let advanced = Cursor {
        seen: vec![Identity::new("b"), Identity::new("a")],
        etag: Some("e2".to_owned()),
        last_modified: None,
    };
    let v2 = version(
        store
            .put_cursor(&source, &advanced, Some(v1))
            .await
            .map_err(|error| failed(&error))?,
    )?;
    ensure(v2 != v1, "a cursor rewritten kept its version")?;
    let lost = store
        .put_cursor(&source, &cursor, Some(v1))
        .await
        .map_err(|error| failed(&error))?;
    ensure(
        lost == Written::Conflict,
        "a cursor was written over a version it did not expect",
    )?;
    let created = store
        .put_cursor(&source, &cursor, None)
        .await
        .map_err(|error| failed(&error))?;
    ensure(
        created == Written::Conflict,
        "a cursor was written as new over one that exists",
    )?;
    let read = store
        .cursor(&source)
        .await
        .map_err(|error| failed(&error))?;
    ensure(
        read.map(|record| record.value) == Some(advanced),
        "a conflicting write changed the cursor",
    )?;
    let other = store
        .cursor(&SourceName::new("other"))
        .await
        .map_err(|error| failed(&error))?;
    ensure(other.is_none(), "writing one cursor made another appear")?;
    Ok(())
}

async fn schedules<S: Store>(store: &S) -> Result<(), String> {
    let schedule = |thread: &str, chore| Schedule {
        key: key(thread),
        thread: thread.to_owned(),
        chore,
        due: NOON,
    };
    let archive = schedule("1", Chore::Archive);
    let lock = schedule("2", Chore::Lock);
    ensure(
        store
            .schedules()
            .await
            .map_err(|error| failed(&error))?
            .is_empty(),
        "a new store holds schedules",
    )?;
    store
        .put_schedules(&[archive.clone(), lock.clone()], &[])
        .await
        .map_err(|error| failed(&error))?;
    store
        .put_schedules(std::slice::from_ref(&archive), &[])
        .await
        .map_err(|error| failed(&error))?;
    let mut held = store.schedules().await.map_err(|error| failed(&error))?;
    held.sort_by(|left, right| left.thread.cmp(&right.thread));
    ensure(
        held == [archive.clone(), lock.clone()],
        "schedules added were not held once each",
    )?;
    store
        .put_schedules(&[], std::slice::from_ref(&archive))
        .await
        .map_err(|error| failed(&error))?;
    store
        .put_schedules(&[], std::slice::from_ref(&archive))
        .await
        .map_err(|error| failed(&error))?;
    let held = store.schedules().await.map_err(|error| failed(&error))?;
    ensure(
        held == [lock],
        "a schedule removed was still held, or another went with it",
    )?;
    Ok(())
}

async fn subscriptions<S: Store>(store: &S) -> Result<(), String> {
    let slack = DestinationName::new("slack");
    let alice = subscription("slack", "herald", "alice");
    let bob = subscription("slack", "herald", "bob");
    let elsewhere = [
        subscription("slack", "other", "carol"),
        subscription("matrix", "herald", "dave"),
    ];
    ensure(
        store
            .subscribers(&slack, "herald")
            .await
            .map_err(|error| failed(&error))?
            .is_empty(),
        "a new store holds subscribers",
    )?;
    for subscribed in [&alice, &bob, &alice, &elsewhere[0], &elsewhere[1]] {
        store
            .subscribe(subscribed)
            .await
            .map_err(|error| failed(&error))?;
    }
    let mut held = store
        .subscribers(&slack, "herald")
        .await
        .map_err(|error| failed(&error))?;
    held.sort();
    ensure(
        held == ["alice", "bob"],
        "the subscribers of a project on a destination were not those subscribed, once each",
    )?;
    store
        .unsubscribe(&alice)
        .await
        .map_err(|error| failed(&error))?;
    store
        .unsubscribe(&alice)
        .await
        .map_err(|error| failed(&error))?;
    let held = store
        .subscribers(&slack, "herald")
        .await
        .map_err(|error| failed(&error))?;
    ensure(
        held == ["bob"],
        "a subscriber removed was still held, or another went with them",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
