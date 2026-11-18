//! Each port of a store, reached through [`DynStore`].

use crate::Decisions;
use herald_core::Decided;
use herald_core::{
    Cursor, DestinationName, Entry, Hold, KeptToken, Key, Latest, Listing, Membership, Pending,
    RouteId, Schedule, SourceName, State, Subscription,
};
use jiff::Timestamp;

use super::DynStore;
use crate::erased::Erased;
use crate::keyring::{Keyring, KnownKey};
use crate::store::{
    Admitted, Announcements, Cursors, Deliveries, Held, Inboxes, Keeper, Keeping, Ledger, Notes,
    Outboxed, Pins, Positions, Schedules, StoreError, Subscriptions, Tokens, Version, Versioned,
    Waiting, Written,
};

impl Ledger for Erased<'_, dyn DynStore + '_> {
    fn keeping(&self) -> Keeping {
        self.0.ledger_keeping()
    }

    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        self.0.entry_boxed(key).await
    }

    async fn put_entry(
        &self,
        entry: &Entry,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_entry_boxed(entry, expected).await
    }

    async fn expire_entries(&self, before: Timestamp) -> Result<Vec<Key>, StoreError> {
        self.0.expire_entries_boxed(before).await
    }
}

impl Cursors for Erased<'_, dyn DynStore + '_> {
    async fn cursor(&self, source: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        self.0.cursor_boxed(source).await
    }

    async fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_cursor_boxed(source, cursor, expected).await
    }

    async fn listing(&self, source: &SourceName) -> Result<Option<Versioned<Listing>>, StoreError> {
        self.0.listing_boxed(source).await
    }

    async fn put_listing(
        &self,
        source: &SourceName,
        listing: &Listing,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_listing_boxed(source, listing, expected).await
    }

    async fn members(&self, source: &SourceName) -> Result<Vec<Versioned<Membership>>, StoreError> {
        self.0.members_boxed(source).await
    }

    async fn member(
        &self,
        source: &SourceName,
        id: &str,
    ) -> Result<Option<Versioned<Membership>>, StoreError> {
        self.0.member_boxed(source, id).await
    }

    async fn put_member(
        &self,
        source: &SourceName,
        membership: &Membership,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_member_boxed(source, membership, expected).await
    }

    async fn forget_member(&self, source: &SourceName, id: &str) -> Result<(), StoreError> {
        self.0.forget_member_boxed(source, id).await
    }
}

impl Schedules for Erased<'_, dyn DynStore + '_> {
    fn keeping(&self) -> Keeping {
        self.0.schedules_keeping()
    }

    async fn every_schedule(&self) -> Result<Vec<Schedule>, StoreError> {
        self.0.every_schedule_boxed().await
    }

    async fn put_schedules(&self, add: &[Schedule], remove: &[Schedule]) -> Result<(), StoreError> {
        self.0.put_schedules_boxed(add, remove).await
    }
}

impl Subscriptions for Erased<'_, dyn DynStore + '_> {
    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<Subscription>, StoreError> {
        self.0.subscribers_boxed(destination, project).await
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.0.subscribe_boxed(subscription).await
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.0.unsubscribe_boxed(subscription).await
    }
}

impl Pins for Erased<'_, dyn DynStore + '_> {
    async fn pinned(&self, route: &RouteId) -> Result<Option<String>, StoreError> {
        self.0.pinned_boxed(route).await
    }

    async fn put_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        self.0.put_pinned_boxed(route, post).await
    }

    async fn forget_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        self.0.forget_pinned_boxed(route, post).await
    }
}

impl Decisions for Erased<'_, dyn DynStore + '_> {
    async fn decision(&self, key: &Key) -> Result<Option<Decided>, StoreError> {
        self.0.decision_boxed(key).await
    }

    async fn put_decision(&self, decided: &Decided) -> Result<(), StoreError> {
        self.0.put_decision_boxed(decided).await
    }

    async fn expire_decisions(&self, before: Timestamp) -> Result<(), StoreError> {
        self.0.expire_decisions_boxed(before).await
    }
}

impl Positions for Erased<'_, dyn DynStore + '_> {
    async fn position(&self, destination: &DestinationName) -> Result<Option<String>, StoreError> {
        self.0.position_boxed(destination).await
    }

    async fn put_position(
        &self,
        destination: &DestinationName,
        position: &str,
    ) -> Result<(), StoreError> {
        self.0.put_position_boxed(destination, position).await
    }
}

impl Tokens for Erased<'_, dyn DynStore + '_> {
    async fn token(&self, name: &str) -> Result<Option<Versioned<KeptToken>>, StoreError> {
        self.0.token_boxed(name).await
    }

    async fn put_token(
        &self,
        token: &KeptToken,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_token_boxed(token, expected).await
    }
}

impl Waiting for Erased<'_, dyn DynStore + '_> {
    async fn pending(&self, hold: &Hold) -> Result<Option<Versioned<Pending>>, StoreError> {
        self.0.pending_boxed(hold).await
    }

    async fn every_pending(&self) -> Result<Vec<Versioned<Pending>>, StoreError> {
        self.0.every_pending_boxed().await
    }

    async fn put_pending(
        &self,
        pending: &Pending,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_pending_boxed(pending, expected).await
    }

    async fn drop_pending(&self, hold: &Hold) -> Result<(), StoreError> {
        self.0.drop_pending_boxed(hold).await
    }
}

impl Deliveries for Erased<'_, dyn DynStore + '_> {
    async fn admit(
        &self,
        namespace: &'static str,
        id: &str,
        at: Timestamp,
    ) -> Result<Admitted, StoreError> {
        self.0.admit_boxed(namespace, id, at).await
    }

    async fn seen(&self, namespace: &'static str, id: &str) -> Result<bool, StoreError> {
        self.0.seen_boxed(namespace, id).await
    }

    async fn release(&self, namespace: &'static str, id: &str) -> Result<(), StoreError> {
        self.0.release_boxed(namespace, id).await
    }

    async fn expire_deliveries(&self, before: Timestamp) -> Result<(), StoreError> {
        self.0.expire_deliveries_boxed(before).await
    }
}

impl Notes for Erased<'_, dyn DynStore + '_> {
    async fn note(&self, key: &Key) -> Result<Option<String>, StoreError> {
        self.0.note_boxed(key).await
    }

    async fn put_note(
        &self,
        actor: &str,
        key: &Key,
        note: &str,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        self.0.put_note_boxed(actor, key, note, at).await
    }

    async fn outbox(
        &self,
        actor: &str,
        after: Option<&str>,
        most: usize,
    ) -> Result<Outboxed, StoreError> {
        self.0.outbox_boxed(actor, after, most).await
    }

    async fn expire_notes(&self, before: Timestamp) -> Result<(), StoreError> {
        self.0.expire_notes_boxed(before).await
    }

    async fn forget_note(&self, key: &Key) -> Result<(), StoreError> {
        self.0.forget_note_boxed(key).await
    }
}

impl Announcements for Erased<'_, dyn DynStore + '_> {
    async fn latest(&self, project: &str) -> Result<Option<Versioned<Latest>>, StoreError> {
        self.0.latest_boxed(project).await
    }

    async fn every_latest(&self) -> Result<Vec<Latest>, StoreError> {
        self.0.every_latest_boxed().await
    }

    async fn put_latest(
        &self,
        latest: &Latest,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_latest_boxed(latest, expected).await
    }

    async fn forget_latest(&self, project: &str) -> Result<(), StoreError> {
        self.0.forget_latest_boxed(project).await
    }
}

impl Inboxes for Erased<'_, dyn DynStore + '_> {
    async fn delivery(&self, key: &Key, inbox: &str) -> Result<Option<State>, StoreError> {
        self.0.delivery_boxed(key, inbox).await
    }

    async fn put_delivery(&self, key: &Key, inbox: &str, state: &State) -> Result<(), StoreError> {
        self.0.put_delivery_boxed(key, inbox, state).await
    }

    async fn forget_deliveries(&self, key: &Key) -> Result<(), StoreError> {
        self.0.forget_deliveries_boxed(key).await
    }
}

impl Keyring for Erased<'_, dyn DynStore + '_> {
    async fn known(&self, id: &str) -> Result<Option<KnownKey>, StoreError> {
        self.0.known_boxed(id).await
    }

    async fn know(&self, id: &str, key: &KnownKey) -> Result<(), StoreError> {
        self.0.know_boxed(id, key).await
    }

    async fn forget_before(&self, before: Timestamp) -> Result<(), StoreError> {
        self.0.forget_before_boxed(before).await
    }
}

impl Keeper for Erased<'_, dyn DynStore + '_> {
    fn holds(&self, held: Held) -> bool {
        self.0.holds(held)
    }

    fn skipped(&self) -> Vec<String> {
        self.0.skipped()
    }

    fn writes_one_at_a_time(&self) -> bool {
        self.0.writes_one_at_a_time()
    }
}
