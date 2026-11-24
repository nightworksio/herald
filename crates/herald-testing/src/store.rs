use herald_core::{
    AdvisedReports, BaseKey, CodeownersKey, Decided, KeptBase, KeptCodeowners, KeptPull, PullKey,
    Reconciled, RepositoryKey, RerunRecord, RunKey,
};
use herald_ports::{Actions, Decisions, Pulls, Recalls};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use herald_core::{
    AuditDay, Cursor, DestinationName, Entry, Feed, Hold, KeptToken, Key, Latest, Listing,
    Membership, PeerKeys, Pending, Publication, PublishedKeys, RouteId, Schedule, SourceName,
    State, Subscription, Window,
};
use herald_ports::{
    Admitted, Announcements, Cursors, Deliveries, Held, Inboxes, Keeper, Ledger, Notes, Outboxed,
    Pins, Positions, Published, Schedules, StoreError, Subscriptions, Tokens, Version, Versioned,
    Waiting, Written,
};
use jiff::Timestamp;

use crate::lock;

#[derive(Debug, Default)]
struct FakeState {
    entries: BTreeMap<Key, Versioned<Entry>>,
    cursors: BTreeMap<SourceName, Versioned<Cursor>>,
    listings: BTreeMap<SourceName, Versioned<Listing>>,
    members: BTreeMap<(SourceName, String), Versioned<Membership>>,
    schedules: Vec<Schedule>,
    subscriptions: BTreeSet<Subscription>,
    pins: BTreeMap<RouteId, String>,
    positions: BTreeMap<DestinationName, String>,
    decisions: BTreeMap<Key, Decided>,
    pulls: BTreeMap<PullKey, Versioned<KeptPull>>,
    reconciled: BTreeMap<SourceName, Versioned<Reconciled>>,
    codeowners: BTreeMap<CodeownersKey, Versioned<KeptCodeowners>>,
    bases: BTreeMap<BaseKey, Versioned<KeptBase>>,
    deliveries: BTreeMap<(&'static str, String), Timestamp>,
    notes: BTreeMap<Key, (String, String, Timestamp)>,
    latest: BTreeMap<String, Versioned<Latest>>,
    inboxes: BTreeMap<(Key, String), State>,
    pending: BTreeMap<Hold, Versioned<Pending>>,
    tokens: BTreeMap<String, Versioned<KeptToken>>,
    published: BTreeMap<(), Versioned<PublishedKeys>>,
    publication: BTreeMap<(), Versioned<Publication>>,
    feeds: BTreeMap<DestinationName, Versioned<Feed>>,
    peer_keys: BTreeMap<String, Versioned<PeerKeys>>,
    audits: BTreeMap<(String, String), Versioned<AuditDay>>,
    windows: BTreeMap<String, Versioned<Window>>,
    recalls: BTreeMap<(), Versioned<AdvisedReports>>,
    actions: BTreeMap<RunKey, Versioned<RerunRecord>>,
    gestures:
        BTreeMap<herald_core::merging::GestureKey, Versioned<herald_core::merging::GestureRecord>>,
}

/// A store in memory, keeping everything a store can keep.
#[derive(Debug, Default)]
pub struct FakeStore {
    state: Mutex<FakeState>,
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

    async fn listing(&self, source: &SourceName) -> Result<Option<Versioned<Listing>>, StoreError> {
        Ok(lock(&self.state).listings.get(source).cloned())
    }

    async fn put_listing(
        &self,
        source: &SourceName,
        listing: &Listing,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.listings,
            source.clone(),
            listing,
            expected,
        ))
    }

    async fn members(&self, source: &SourceName) -> Result<Vec<Versioned<Membership>>, StoreError> {
        Ok(lock(&self.state)
            .members
            .iter()
            .filter(|((owner, _), _)| owner == source)
            .map(|(_, membership)| membership.clone())
            .collect())
    }

    async fn member(
        &self,
        source: &SourceName,
        id: &str,
    ) -> Result<Option<Versioned<Membership>>, StoreError> {
        Ok(lock(&self.state)
            .members
            .get(&(source.clone(), id.to_owned()))
            .cloned())
    }

    async fn put_member(
        &self,
        source: &SourceName,
        membership: &Membership,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        let key = (source.clone(), membership.repository.id.clone());
        Ok(compare_and_set(
            &mut state.members,
            key,
            membership,
            expected,
        ))
    }

    async fn forget_member(&self, source: &SourceName, id: &str) -> Result<(), StoreError> {
        lock(&self.state)
            .members
            .remove(&(source.clone(), id.to_owned()));
        Ok(())
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

    async fn forget_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        let mut state = lock(&self.state);
        if state.pins.get(route).is_some_and(|pinned| pinned == post) {
            state.pins.remove(route);
        }
        Ok(())
    }
}

impl Decisions for FakeStore {
    async fn decision(&self, key: &Key) -> Result<Option<Decided>, StoreError> {
        Ok(lock(&self.state).decisions.get(key).cloned())
    }

    async fn put_decision(&self, decided: &Decided) -> Result<(), StoreError> {
        lock(&self.state)
            .decisions
            .insert(decided.key, decided.clone());
        Ok(())
    }

    async fn expire_decisions(&self, before: Timestamp) -> Result<(), StoreError> {
        lock(&self.state)
            .decisions
            .retain(|_, decided| decided.at >= before);
        Ok(())
    }
}

impl Positions for FakeStore {
    async fn position(&self, destination: &DestinationName) -> Result<Option<String>, StoreError> {
        Ok(lock(&self.state).positions.get(destination).cloned())
    }

    async fn put_position(
        &self,
        destination: &DestinationName,
        position: &str,
    ) -> Result<(), StoreError> {
        lock(&self.state)
            .positions
            .insert(destination.clone(), position.to_owned());
        Ok(())
    }
}

impl Deliveries for FakeStore {
    async fn admit(
        &self,
        namespace: &'static str,
        id: &str,
        at: Timestamp,
    ) -> Result<Admitted, StoreError> {
        let mut state = lock(&self.state);
        let delivery = (namespace, id.to_owned());
        if state.deliveries.contains_key(&delivery) {
            return Ok(Admitted::Seen);
        }
        state.deliveries.insert(delivery, at);
        Ok(Admitted::Fresh)
    }

    async fn seen(&self, namespace: &'static str, id: &str) -> Result<bool, StoreError> {
        Ok(lock(&self.state)
            .deliveries
            .contains_key(&(namespace, id.to_owned())))
    }

    async fn release(&self, namespace: &'static str, id: &str) -> Result<(), StoreError> {
        lock(&self.state)
            .deliveries
            .remove(&(namespace, id.to_owned()));
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

    async fn forget_note(&self, key: &Key) -> Result<(), StoreError> {
        lock(&self.state).notes.remove(key);
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

    async fn forget_latest(&self, project: &str) -> Result<(), StoreError> {
        lock(&self.state).latest.remove(project);
        Ok(())
    }
}

impl Waiting for FakeStore {
    async fn pending(&self, hold: &Hold) -> Result<Option<Versioned<Pending>>, StoreError> {
        Ok(lock(&self.state).pending.get(hold).cloned())
    }

    async fn every_pending(&self) -> Result<Vec<Versioned<Pending>>, StoreError> {
        Ok(lock(&self.state).pending.values().cloned().collect())
    }

    async fn put_pending(
        &self,
        pending: &Pending,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.pending,
            pending.hold.clone(),
            pending,
            expected,
        ))
    }

    async fn drop_pending(&self, hold: &Hold) -> Result<(), StoreError> {
        lock(&self.state).pending.remove(hold);
        Ok(())
    }
}

impl Tokens for FakeStore {
    async fn token(&self, name: &str) -> Result<Option<Versioned<KeptToken>>, StoreError> {
        Ok(lock(&self.state).tokens.get(name).cloned())
    }

    async fn put_token(
        &self,
        token: &KeptToken,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.tokens,
            token.name.clone(),
            token,
            expected,
        ))
    }
}

impl Published for FakeStore {
    async fn published_keys(&self) -> Result<Option<Versioned<PublishedKeys>>, StoreError> {
        Ok(lock(&self.state).published.get(&()).cloned())
    }

    async fn put_published_keys(
        &self,
        keys: &PublishedKeys,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(&mut state.published, (), keys, expected))
    }

    async fn publication(&self) -> Result<Option<Versioned<Publication>>, StoreError> {
        Ok(lock(&self.state).publication.get(&()).cloned())
    }

    async fn put_publication(
        &self,
        publication: &Publication,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.publication,
            (),
            publication,
            expected,
        ))
    }

    async fn feed(
        &self,
        destination: &DestinationName,
    ) -> Result<Option<Versioned<Feed>>, StoreError> {
        Ok(lock(&self.state).feeds.get(destination).cloned())
    }

    async fn put_feed(
        &self,
        destination: &DestinationName,
        feed: &Feed,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.feeds,
            destination.clone(),
            feed,
            expected,
        ))
    }

    async fn peer_keys(&self, peer: &str) -> Result<Option<Versioned<PeerKeys>>, StoreError> {
        Ok(lock(&self.state).peer_keys.get(peer).cloned())
    }

    async fn put_peer_keys(
        &self,
        peer: &str,
        keys: &PeerKeys,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.peer_keys,
            peer.to_owned(),
            keys,
            expected,
        ))
    }

    async fn audit(
        &self,
        peer: &str,
        day: &str,
    ) -> Result<Option<Versioned<AuditDay>>, StoreError> {
        Ok(lock(&self.state)
            .audits
            .get(&(peer.to_owned(), day.to_owned()))
            .cloned())
    }

    async fn put_audit(
        &self,
        audit: &AuditDay,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.audits,
            (audit.peer.clone(), audit.day.clone()),
            audit,
            expected,
        ))
    }

    async fn audits(&self, peer: &str) -> Result<Vec<AuditDay>, StoreError> {
        Ok(lock(&self.state)
            .audits
            .iter()
            .filter(|((kept, _), _)| kept == peer)
            .map(|(_, audit)| audit.value.clone())
            .collect())
    }

    async fn expire_audits(&self, day: &str) -> Result<(), StoreError> {
        lock(&self.state)
            .audits
            .retain(|(_, kept), _| kept.as_str() >= day);
        Ok(())
    }

    async fn window(&self, peer: &str) -> Result<Option<Versioned<Window>>, StoreError> {
        Ok(lock(&self.state).windows.get(peer).cloned())
    }

    async fn put_window(
        &self,
        peer: &str,
        window: &Window,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.windows,
            peer.to_owned(),
            window,
            expected,
        ))
    }
}

impl Actions for FakeStore {
    async fn action(&self, key: &RunKey) -> Result<Option<Versioned<RerunRecord>>, StoreError> {
        Ok(lock(&self.state).actions.get(key).cloned())
    }

    async fn put_action(
        &self,
        action: &RerunRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.actions,
            action.key.clone(),
            action,
            expected,
        ))
    }

    async fn gesture(
        &self,
        key: &herald_core::merging::GestureKey,
    ) -> Result<Option<Versioned<herald_core::merging::GestureRecord>>, StoreError> {
        Ok(lock(&self.state).gestures.get(key).cloned())
    }

    async fn put_gesture(
        &self,
        gesture: &herald_core::merging::GestureRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.gestures,
            gesture.key.clone(),
            gesture,
            expected,
        ))
    }
}

impl Recalls for FakeStore {
    async fn advised(&self) -> Result<Option<Versioned<AdvisedReports>>, StoreError> {
        Ok(lock(&self.state).recalls.get(&()).cloned())
    }

    async fn put_advised(
        &self,
        reports: &AdvisedReports,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(&mut state.recalls, (), reports, expected))
    }
}

impl Pulls for FakeStore {
    async fn pull(&self, key: &PullKey) -> Result<Option<Versioned<KeptPull>>, StoreError> {
        Ok(lock(&self.state).pulls.get(key).cloned())
    }

    async fn pulls_of(
        &self,
        repository: &RepositoryKey,
    ) -> Result<Vec<Versioned<KeptPull>>, StoreError> {
        Ok(lock(&self.state)
            .pulls
            .iter()
            .filter(|(key, _)| key.repository == *repository)
            .map(|(_, kept)| kept.clone())
            .collect())
    }

    async fn put_pull(
        &self,
        kept: &KeptPull,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.pulls,
            kept.key.clone(),
            kept,
            expected,
        ))
    }

    async fn expire_pulls(&self, before: Timestamp) -> Result<(), StoreError> {
        lock(&self.state)
            .pulls
            .retain(|_, kept| kept.value.closed.is_none_or(|closed| closed.at >= before));
        lock(&self.state)
            .codeowners
            .retain(|_, kept| kept.value.at >= before);
        lock(&self.state)
            .bases
            .retain(|_, kept| kept.value.at >= before);
        Ok(())
    }

    async fn reconciled(
        &self,
        source: &SourceName,
    ) -> Result<Option<Versioned<Reconciled>>, StoreError> {
        Ok(lock(&self.state).reconciled.get(source).cloned())
    }

    async fn put_reconciled(
        &self,
        source: &SourceName,
        reconciled: &Reconciled,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.reconciled,
            source.clone(),
            reconciled,
            expected,
        ))
    }

    async fn codeowners(
        &self,
        key: &CodeownersKey,
    ) -> Result<Option<Versioned<KeptCodeowners>>, StoreError> {
        Ok(lock(&self.state).codeowners.get(key).cloned())
    }

    async fn put_codeowners(
        &self,
        kept: &KeptCodeowners,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.codeowners,
            kept.key.clone(),
            kept,
            expected,
        ))
    }

    async fn base(&self, key: &BaseKey) -> Result<Option<Versioned<KeptBase>>, StoreError> {
        Ok(lock(&self.state).bases.get(key).cloned())
    }

    async fn put_base(
        &self,
        kept: &KeptBase,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let mut state = lock(&self.state);
        Ok(compare_and_set(
            &mut state.bases,
            kept.key.clone(),
            kept,
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
        assert!(itself(store.positions()));
        assert!(itself(store.decisions()));
        assert!(itself(store.pulls()));
        assert!(itself(store.recalls()));
        assert!(itself(store.actions()));
        assert!(itself(store.waiting()));
    }
}
