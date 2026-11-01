use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use herald_core::{Cursor, DestinationName, Entry, Key, Schedule, SourceName, Subscription};
use herald_ports::{Capabilities, Store, StoreError, Version, Versioned, Written};

use crate::lock;

#[derive(Debug, Default)]
struct State {
    entries: BTreeMap<Key, Versioned<Entry>>,
    cursors: BTreeMap<SourceName, Versioned<Cursor>>,
    schedules: Vec<Schedule>,
    subscriptions: BTreeSet<Subscription>,
}

/// A store in memory, keeping everything a store can keep.
#[derive(Debug, Default)]
pub struct FakeStore {
    state: Mutex<State>,
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

impl Store for FakeStore {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            ledger: true,
            cursors: true,
            schedules: true,
            subscriptions: true,
            concurrent: true,
        }
    }

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

    async fn schedules(&self) -> Result<Vec<Schedule>, StoreError> {
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

    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<String>, StoreError> {
        Ok(lock(&self.state)
            .subscriptions
            .iter()
            .filter(|subscription| {
                &subscription.destination == destination && subscription.project == project
            })
            .map(|subscription| subscription.subscriber.clone())
            .collect())
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        lock(&self.state).subscriptions.insert(subscription.clone());
        Ok(())
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        lock(&self.state).subscriptions.remove(subscription);
        Ok(())
    }
}
