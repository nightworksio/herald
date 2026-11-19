//! Each kind of record answered through an accessor's `Option`: the kept
//! records where the store keeps the kind, and [`Unkept`]'s refusal where it
//! does not.

use crate::{Decisions, Pulls};
use herald_core::{
    Cursor, DestinationName, Entry, Hold, Key, Latest, Listing, Membership, Pending, RouteId,
    Schedule, SourceName, State, Subscription,
};
use herald_core::{Decided, KeptPull, PullKey, Reconciled, RepositoryKey};
use jiff::Timestamp;

use super::{
    Admitted, Announcements, Cursors, Deliveries, Inboxes, Keeping, Ledger, Notes, Outboxed, Pins,
    Positions, Schedules, StoreError, Subscriptions, Unkept, Version, Versioned, Waiting, Written,
};

impl<T: Ledger> Ledger for Option<&T> {
    fn keeping(&self) -> Keeping {
        self.map_or(Keeping::Kept, Ledger::keeping)
    }

    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        match self {
            Some(kept) => kept.entry(key).await,
            None => Unkept.entry(key).await,
        }
    }

    async fn put_entry(
        &self,
        entry: &Entry,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_entry(entry, expected).await,
            None => Unkept.put_entry(entry, expected).await,
        }
    }

    async fn expire_entries(&self, before: Timestamp) -> Result<Vec<Key>, StoreError> {
        match self {
            Some(kept) => kept.expire_entries(before).await,
            None => Unkept.expire_entries(before).await,
        }
    }
}

impl<T: Cursors> Cursors for Option<&T> {
    async fn cursor(&self, source: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        match self {
            Some(kept) => kept.cursor(source).await,
            None => Unkept.cursor(source).await,
        }
    }

    async fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_cursor(source, cursor, expected).await,
            None => Unkept.put_cursor(source, cursor, expected).await,
        }
    }

    async fn listing(&self, source: &SourceName) -> Result<Option<Versioned<Listing>>, StoreError> {
        match self {
            Some(kept) => kept.listing(source).await,
            None => Unkept.listing(source).await,
        }
    }

    async fn put_listing(
        &self,
        source: &SourceName,
        listing: &Listing,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_listing(source, listing, expected).await,
            None => Unkept.put_listing(source, listing, expected).await,
        }
    }

    async fn members(&self, source: &SourceName) -> Result<Vec<Versioned<Membership>>, StoreError> {
        match self {
            Some(kept) => kept.members(source).await,
            None => Unkept.members(source).await,
        }
    }

    async fn put_member(
        &self,
        source: &SourceName,
        membership: &Membership,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_member(source, membership, expected).await,
            None => Unkept.put_member(source, membership, expected).await,
        }
    }

    async fn member(
        &self,
        source: &SourceName,
        id: &str,
    ) -> Result<Option<Versioned<Membership>>, StoreError> {
        match self {
            Some(kept) => kept.member(source, id).await,
            None => Unkept.member(source, id).await,
        }
    }

    async fn forget_member(&self, source: &SourceName, id: &str) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.forget_member(source, id).await,
            None => Unkept.forget_member(source, id).await,
        }
    }
}

impl<T: Schedules> Schedules for Option<&T> {
    fn keeping(&self) -> Keeping {
        self.map_or(Keeping::Kept, Schedules::keeping)
    }

    async fn every_schedule(&self) -> Result<Vec<Schedule>, StoreError> {
        match self {
            Some(kept) => kept.every_schedule().await,
            None => Unkept.every_schedule().await,
        }
    }

    async fn put_schedules(&self, add: &[Schedule], remove: &[Schedule]) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.put_schedules(add, remove).await,
            None => Unkept.put_schedules(add, remove).await,
        }
    }
}

impl<T: Subscriptions> Subscriptions for Option<&T> {
    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<Subscription>, StoreError> {
        match self {
            Some(kept) => kept.subscribers(destination, project).await,
            None => Unkept.subscribers(destination, project).await,
        }
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.subscribe(subscription).await,
            None => Unkept.subscribe(subscription).await,
        }
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.unsubscribe(subscription).await,
            None => Unkept.unsubscribe(subscription).await,
        }
    }
}

impl<T: Decisions> Decisions for Option<&T> {
    async fn decision(&self, key: &Key) -> Result<Option<Decided>, StoreError> {
        match self {
            Some(kept) => kept.decision(key).await,
            None => Unkept.decision(key).await,
        }
    }

    async fn put_decision(&self, decided: &Decided) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.put_decision(decided).await,
            None => Unkept.put_decision(decided).await,
        }
    }

    async fn expire_decisions(&self, before: Timestamp) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.expire_decisions(before).await,
            None => Unkept.expire_decisions(before).await,
        }
    }
}

impl<T: Pulls> Pulls for Option<&T> {
    async fn pull(&self, key: &PullKey) -> Result<Option<Versioned<KeptPull>>, StoreError> {
        match self {
            Some(kept) => kept.pull(key).await,
            None => Unkept.pull(key).await,
        }
    }

    async fn pulls_of(
        &self,
        repository: &RepositoryKey,
    ) -> Result<Vec<Versioned<KeptPull>>, StoreError> {
        match self {
            Some(kept) => kept.pulls_of(repository).await,
            None => Unkept.pulls_of(repository).await,
        }
    }

    async fn put_pull(
        &self,
        pull: &KeptPull,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_pull(pull, expected).await,
            None => Unkept.put_pull(pull, expected).await,
        }
    }

    async fn expire_pulls(&self, before: Timestamp) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.expire_pulls(before).await,
            None => Unkept.expire_pulls(before).await,
        }
    }

    async fn reconciled(
        &self,
        source: &SourceName,
    ) -> Result<Option<Versioned<Reconciled>>, StoreError> {
        match self {
            Some(kept) => kept.reconciled(source).await,
            None => Unkept.reconciled(source).await,
        }
    }

    async fn put_reconciled(
        &self,
        source: &SourceName,
        reconciled: &Reconciled,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_reconciled(source, reconciled, expected).await,
            None => Unkept.put_reconciled(source, reconciled, expected).await,
        }
    }
}

impl<T: Positions> Positions for Option<&T> {
    async fn position(&self, destination: &DestinationName) -> Result<Option<String>, StoreError> {
        match self {
            Some(kept) => kept.position(destination).await,
            None => Unkept.position(destination).await,
        }
    }

    async fn put_position(
        &self,
        destination: &DestinationName,
        position: &str,
    ) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.put_position(destination, position).await,
            None => Unkept.put_position(destination, position).await,
        }
    }
}

impl<T: Waiting> Waiting for Option<&T> {
    async fn pending(&self, hold: &Hold) -> Result<Option<Versioned<Pending>>, StoreError> {
        match self {
            Some(kept) => kept.pending(hold).await,
            None => Unkept.pending(hold).await,
        }
    }

    async fn every_pending(&self) -> Result<Vec<Versioned<Pending>>, StoreError> {
        match self {
            Some(kept) => kept.every_pending().await,
            None => Unkept.every_pending().await,
        }
    }

    async fn put_pending(
        &self,
        pending: &Pending,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_pending(pending, expected).await,
            None => Unkept.put_pending(pending, expected).await,
        }
    }

    async fn drop_pending(&self, hold: &Hold) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.drop_pending(hold).await,
            None => Unkept.drop_pending(hold).await,
        }
    }
}

impl<T: Pins> Pins for Option<&T> {
    async fn pinned(&self, route: &RouteId) -> Result<Option<String>, StoreError> {
        match self {
            Some(kept) => kept.pinned(route).await,
            None => Unkept.pinned(route).await,
        }
    }

    async fn put_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.put_pinned(route, post).await,
            None => Unkept.put_pinned(route, post).await,
        }
    }

    async fn forget_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.forget_pinned(route, post).await,
            None => Unkept.forget_pinned(route, post).await,
        }
    }
}

impl<T: Deliveries> Deliveries for Option<&T> {
    async fn admit(
        &self,
        namespace: &'static str,
        id: &str,
        at: Timestamp,
    ) -> Result<Admitted, StoreError> {
        match self {
            Some(kept) => kept.admit(namespace, id, at).await,
            None => Unkept.admit(namespace, id, at).await,
        }
    }

    async fn seen(&self, namespace: &'static str, id: &str) -> Result<bool, StoreError> {
        match self {
            Some(kept) => kept.seen(namespace, id).await,
            None => Unkept.seen(namespace, id).await,
        }
    }

    async fn release(&self, namespace: &'static str, id: &str) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.release(namespace, id).await,
            None => Unkept.release(namespace, id).await,
        }
    }

    async fn expire_deliveries(&self, before: Timestamp) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.expire_deliveries(before).await,
            None => Unkept.expire_deliveries(before).await,
        }
    }
}

impl<T: Notes> Notes for Option<&T> {
    async fn note(&self, key: &Key) -> Result<Option<String>, StoreError> {
        match self {
            Some(kept) => kept.note(key).await,
            None => Unkept.note(key).await,
        }
    }

    async fn put_note(
        &self,
        actor: &str,
        key: &Key,
        note: &str,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.put_note(actor, key, note, at).await,
            None => Unkept.put_note(actor, key, note, at).await,
        }
    }

    async fn outbox(
        &self,
        actor: &str,
        after: Option<&str>,
        most: usize,
    ) -> Result<Outboxed, StoreError> {
        match self {
            Some(kept) => kept.outbox(actor, after, most).await,
            None => Unkept.outbox(actor, after, most).await,
        }
    }

    async fn expire_notes(&self, before: Timestamp) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.expire_notes(before).await,
            None => Unkept.expire_notes(before).await,
        }
    }

    async fn forget_note(&self, key: &Key) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.forget_note(key).await,
            None => Unkept.forget_note(key).await,
        }
    }
}

impl<T: Announcements> Announcements for Option<&T> {
    async fn latest(&self, project: &str) -> Result<Option<Versioned<Latest>>, StoreError> {
        match self {
            Some(kept) => kept.latest(project).await,
            None => Unkept.latest(project).await,
        }
    }

    async fn every_latest(&self) -> Result<Vec<Latest>, StoreError> {
        match self {
            Some(kept) => kept.every_latest().await,
            None => Unkept.every_latest().await,
        }
    }

    async fn put_latest(
        &self,
        latest: &Latest,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_latest(latest, expected).await,
            None => Unkept.put_latest(latest, expected).await,
        }
    }

    async fn forget_latest(&self, project: &str) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.forget_latest(project).await,
            None => Unkept.forget_latest(project).await,
        }
    }
}

impl<T: Inboxes> Inboxes for Option<&T> {
    async fn delivery(&self, key: &Key, inbox: &str) -> Result<Option<State>, StoreError> {
        match self {
            Some(kept) => kept.delivery(key, inbox).await,
            None => Unkept.delivery(key, inbox).await,
        }
    }

    async fn put_delivery(&self, key: &Key, inbox: &str, state: &State) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.put_delivery(key, inbox, state).await,
            None => Unkept.put_delivery(key, inbox, state).await,
        }
    }

    async fn forget_deliveries(&self, key: &Key) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.forget_deliveries(key).await,
            None => Unkept.forget_deliveries(key).await,
        }
    }
}
