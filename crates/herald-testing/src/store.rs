use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use herald_core::{
    Cursor, DestinationName, Entry, Key, Latest, RouteId, Schedule, Scheme, SourceName, State,
    Subscription,
};
use herald_ports::{
    Admitted, Announcements, Cursors, Deliveries, Held, Inboxes, Keeper, Ledger, Notes, Outboxed,
    Pins, Schedules, StoreError, Subscriptions, Version, Versioned, Written,
};
use jiff::Timestamp;

use crate::lock;

#[derive(Debug, Default)]
struct Records {
    entries: BTreeMap<Key, Versioned<Entry>>,
    cursors: BTreeMap<SourceName, Versioned<Cursor>>,
    schedules: Vec<Schedule>,
    subscriptions: BTreeSet<Subscription>,
    pins: BTreeMap<RouteId, String>,
    deliveries: BTreeMap<(&'static str, String), Timestamp>,
    notes: BTreeMap<Key, (String, String, Timestamp)>,
    latest: BTreeMap<String, Versioned<Latest>>,
    inboxes: BTreeMap<(Key, String), State>,
}

/// A store in memory, keeping everything a store can keep.
#[derive(Debug, Default)]
pub struct FakeStore {
    state: Mutex<Records>,
}

fn compare_and_set<K: Ord, T: Clone>(
    records: &mut BTreeMap<K, Versioned<T>>,
    key: K,
    value: &T,
    expected: Option<Version>,
) -> Written {
    let current = records.get(&key).map(|record| record.version);
    if current != expected {
        return Written::Conflict;
    }
    let version = Version(current.map_or(1, |Version(number)| number + 1));
    records.insert(
        key,
        Versioned {
            value: value.clone(),
            version,
        },
    );
    Written::Written(version)
}

impl Inboxes for FakeStore {
    async fn delivery(&self, key: &Key, inbox: &str) -> Result<Option<State>, StoreError> {
        Ok(lock(&self.state)
            .inboxes
            .get(&(*key, inbox.to_owned()))
            .cloned())
    }

    async fn put_delivery(&self, key: &Key, inbox: &str, state: &State) -> Result<(), StoreError> {
        lock(&self.state)
            .inboxes
            .insert((*key, inbox.to_owned()), state.clone());
        Ok(())
    }

    async fn forget_deliveries(&self, key: &Key) -> Result<(), StoreError> {
        lock(&self.state)
            .inboxes
            .retain(|(delivered, _), _| delivered != key);
        Ok(())
    }
}

impl Ledger for FakeStore {
    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        Ok(lock(&self.state).entries.get(key).cloned())
    }

    async fn put_entry(
        &self,
        entry: &Entry,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.entries,
            entry.key,
            entry,
            expected,
        ))
    }

    async fn expire_entries(&self, before: Timestamp) -> Result<Vec<Key>, StoreError> {
        let mut state = lock(&self.state);
        let expired: Vec<Key> = state
            .entries
            .values()
            .filter(|entry| entry.value.expired(before))
            .map(|entry| entry.value.key)
            .collect();
        state.entries.retain(|key, _| !expired.contains(key));
        Ok(expired)
    }
}

impl Cursors for FakeStore {
    async fn cursor(&self, source: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        Ok(lock(&self.state).cursors.get(source).cloned())
    }

    async fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.cursors,
            source.clone(),
            cursor,
            expected,
        ))
    }
}

impl Schedules for FakeStore {
    async fn every_schedule(&self) -> Result<Vec<Schedule>, StoreError> {
        Ok(lock(&self.state).schedules.clone())
    }

    async fn put_schedules(&self, add: &[Schedule], remove: &[Schedule]) -> Result<(), StoreError> {
        let mut state = lock(&self.state);
        state
            .schedules
            .retain(|schedule| !remove.contains(schedule));
        for schedule in add {
            if !state.schedules.contains(schedule) {
                state.schedules.push(schedule.clone());
            }
        }
        Ok(())
    }
}

impl Subscriptions for FakeStore {
    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<Subscription>, StoreError> {
        Ok(lock(&self.state)
            .subscriptions
            .iter()
            .filter(|subscription| {
                &subscription.destination == destination && subscription.project == project
            })
            .cloned()
            .collect())
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        let mut state = lock(&self.state);
        state
            .subscriptions
            .retain(|kept| !same_subscriber(kept, subscription));
        state.subscriptions.insert(subscription.clone());
        Ok(())
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        lock(&self.state)
            .subscriptions
            .retain(|kept| !same_subscriber(kept, subscription));
        Ok(())
    }
}

/// Whether `one` and `other` are one subscriber's to one project on one
/// destination.
fn same_subscriber(one: &Subscription, other: &Subscription) -> bool {
    one.destination == other.destination
        && one.project == other.project
        && one.subscriber == other.subscriber
}

impl Pins for FakeStore {
    async fn pinned(&self, route: &RouteId) -> Result<Option<String>, StoreError> {
        Ok(lock(&self.state).pins.get(route).cloned())
    }

    async fn put_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        lock(&self.state)
            .pins
            .insert(route.clone(), post.to_owned());
        Ok(())
    }
}

impl Deliveries for FakeStore {
    async fn admit(&self, scheme: Scheme, id: &str, at: Timestamp) -> Result<Admitted, StoreError> {
        let mut state = lock(&self.state);
        let delivery = (scheme.name(), id.to_owned());
        if state.deliveries.contains_key(&delivery) {
            return Ok(Admitted::Seen);
        }
        state.deliveries.insert(delivery, at);
        Ok(Admitted::Fresh)
    }

    async fn release(&self, scheme: Scheme, id: &str) -> Result<(), StoreError> {
        lock(&self.state)
            .deliveries
            .remove(&(scheme.name(), id.to_owned()));
        Ok(())
    }

    async fn expire_deliveries(&self, before: Timestamp) -> Result<(), StoreError> {
        lock(&self.state).deliveries.retain(|_, at| *at >= before);
        Ok(())
    }
}

impl Notes for FakeStore {
    async fn note(&self, key: &Key) -> Result<Option<String>, StoreError> {
        Ok(lock(&self.state)
            .notes
            .get(key)
            .map(|(_, note, _)| note.clone()))
    }

    async fn put_note(
        &self,
        actor: &str,
        key: &Key,
        note: &str,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        lock(&self.state)
            .notes
            .insert(*key, (actor.to_owned(), note.to_owned(), at));
        Ok(())
    }

    async fn outbox(
        &self,
        actor: &str,
        after: Option<&str>,
        most: usize,
    ) -> Result<Outboxed, StoreError> {
        let listed = lock(&self.state)
            .notes
            .iter()
            .filter(|(_, (by, _, _))| by == actor)
            .map(|(key, (_, note, at))| (Outboxed::name(*at, key), note.clone()))
            .collect();
        Ok(Outboxed::page(listed, after, most))
    }

    async fn expire_notes(&self, before: Timestamp) -> Result<(), StoreError> {
        lock(&self.state)
            .notes
            .retain(|_, (_, _, at)| *at >= before);
        Ok(())
    }
}

impl Announcements for FakeStore {
    async fn latest(&self, project: &str) -> Result<Option<Versioned<Latest>>, StoreError> {
        Ok(lock(&self.state).latest.get(project).cloned())
    }

    async fn every_latest(&self) -> Result<Vec<Latest>, StoreError> {
        Ok(lock(&self.state)
            .latest
            .values()
            .map(|kept| kept.value.clone())
            .collect())
    }

    async fn put_latest(
        &self,
        latest: &Latest,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        Ok(compare_and_set(
            &mut lock(&self.state).latest,
            latest.project.clone(),
            latest,
            expected,
        ))
    }
}

impl Keeper for FakeStore {
    fn holds(&self, _: Held) -> bool {
        true
    }

    fn skipped(&self) -> Vec<String> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use herald_ports::Store;

    #[test]
    fn the_store_answers_every_kind_itself() {
        let store = FakeStore::default();
        let itself = |answered: Option<&FakeStore>| {
            answered.is_some_and(|kept| std::ptr::eq(kept, &raw const store))
        };
        assert!(itself(store.ledger()));
        assert!(itself(store.cursors()));
        assert!(itself(store.schedules()));
        assert!(itself(store.subscriptions()));
        assert!(itself(store.pins()));
        assert!(itself(store.deliveries()));
        assert!(itself(store.notes()));
        assert!(itself(store.announcements()));
        assert!(itself(store.inboxes()));
    }
}
