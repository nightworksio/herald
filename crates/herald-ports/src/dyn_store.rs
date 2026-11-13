use herald_core::{
    Cursor, DestinationName, Entry, Key, Latest, RouteId, Schedule, Scheme, SourceName, State,
    Subscription,
};
use jiff::Timestamp;

use crate::erased::{Boxed, Erased};
use crate::keyring::{Keyring, KnownKey};
use crate::store::{
    Admitted, Announcements, Cursors, Deliveries, Held, Inboxes, Keeper, Keeping, Ledger, Notes,
    Outboxed, Pins, Positions, Schedules, Store, StoreError, Subscriptions, Version, Versioned,
    Written,
};

/// [`Store`] and [`Keyring`] as one trait object: every store that is both
/// is one.
pub trait DynStore: Sync {
    /// Whether it holds records of `held`.
    fn holds(&self, held: Held) -> bool;

    /// The paths of the records a listing skipped as corrupt since this was
    /// last asked, each once.
    fn skipped(&self) -> Vec<String>;

    /// [`Store::writes_one_at_a_time`].
    fn writes_one_at_a_time(&self) -> bool;

    /// How its ledger is held.
    fn ledger_keeping(&self) -> Keeping;

    /// [`Ledger::entry`].
    fn entry_boxed<'a>(
        &'a self,
        key: &'a Key,
    ) -> Boxed<'a, Result<Option<Versioned<Entry>>, StoreError>>;

    /// [`Ledger::put_entry`].
    fn put_entry_boxed<'a>(
        &'a self,
        entry: &'a Entry,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Ledger::expire_entries`].
    fn expire_entries_boxed(&self, before: Timestamp) -> Boxed<'_, Result<Vec<Key>, StoreError>>;

    /// [`Cursors::cursor`].
    fn cursor_boxed<'a>(
        &'a self,
        source: &'a SourceName,
    ) -> Boxed<'a, Result<Option<Versioned<Cursor>>, StoreError>>;

    /// [`Cursors::put_cursor`].
    fn put_cursor_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        cursor: &'a Cursor,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// How its schedules are held.
    fn schedules_keeping(&self) -> Keeping;

    /// [`Schedules::every_schedule`].
    fn every_schedule_boxed(&self) -> Boxed<'_, Result<Vec<Schedule>, StoreError>>;

    /// [`Schedules::put_schedules`].
    fn put_schedules_boxed<'a>(
        &'a self,
        add: &'a [Schedule],
        remove: &'a [Schedule],
    ) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Subscriptions::subscribers`].
    fn subscribers_boxed<'a>(
        &'a self,
        destination: &'a DestinationName,
        project: &'a str,
    ) -> Boxed<'a, Result<Vec<Subscription>, StoreError>>;

    /// [`Subscriptions::subscribe`].
    fn subscribe_boxed<'a>(
        &'a self,
        subscription: &'a Subscription,
    ) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Subscriptions::unsubscribe`].
    fn unsubscribe_boxed<'a>(
        &'a self,
        subscription: &'a Subscription,
    ) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Pins::pinned`].
    fn pinned_boxed<'a>(
        &'a self,
        route: &'a RouteId,
    ) -> Boxed<'a, Result<Option<String>, StoreError>>;

    /// [`Pins::put_pinned`].
    fn put_pinned_boxed<'a>(
        &'a self,
        route: &'a RouteId,
        post: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Positions::position`].
    fn position_boxed<'a>(
        &'a self,
        destination: &'a DestinationName,
    ) -> Boxed<'a, Result<Option<String>, StoreError>>;

    /// [`Positions::put_position`].
    fn put_position_boxed<'a>(
        &'a self,
        destination: &'a DestinationName,
        position: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Deliveries::admit`].
    fn admit_boxed<'a>(
        &'a self,
        scheme: Scheme,
        id: &'a str,
        at: Timestamp,
    ) -> Boxed<'a, Result<Admitted, StoreError>>;

    /// [`Deliveries::release`].
    fn release_boxed<'a>(
        &'a self,
        scheme: Scheme,
        id: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Deliveries::expire_deliveries`].
    fn expire_deliveries_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>>;

    /// [`Notes::note`].
    fn note_boxed<'a>(&'a self, key: &'a Key) -> Boxed<'a, Result<Option<String>, StoreError>>;

    /// [`Notes::put_note`].
    fn put_note_boxed<'a>(
        &'a self,
        actor: &'a str,
        key: &'a Key,
        note: &'a str,
        at: Timestamp,
    ) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Notes::outbox`].
    fn outbox_boxed<'a>(
        &'a self,
        actor: &'a str,
        after: Option<&'a str>,
        most: usize,
    ) -> Boxed<'a, Result<Outboxed, StoreError>>;

    /// [`Notes::expire_notes`].
    fn expire_notes_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>>;

    /// [`Announcements::latest`].
    fn latest_boxed<'a>(
        &'a self,
        project: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<Latest>>, StoreError>>;

    /// [`Announcements::every_latest`].
    fn every_latest_boxed(&self) -> Boxed<'_, Result<Vec<Latest>, StoreError>>;

    /// [`Announcements::put_latest`].
    fn put_latest_boxed<'a>(
        &'a self,
        latest: &'a Latest,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Inboxes::delivery`].
    fn delivery_boxed<'a>(
        &'a self,
        key: &'a Key,
        inbox: &'a str,
    ) -> Boxed<'a, Result<Option<State>, StoreError>>;

    /// [`Inboxes::put_delivery`].
    fn put_delivery_boxed<'a>(
        &'a self,
        key: &'a Key,
        inbox: &'a str,
        state: &'a State,
    ) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Inboxes::forget_deliveries`].
    fn forget_deliveries_boxed<'a>(&'a self, key: &'a Key) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Keyring::known`].
    fn known_boxed<'a>(&'a self, id: &'a str) -> Boxed<'a, Result<Option<KnownKey>, StoreError>>;

    /// [`Keyring::know`].
    fn know_boxed<'a>(
        &'a self,
        id: &'a str,
        key: &'a KnownKey,
    ) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Keyring::forget_before`].
    fn forget_before_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>>;
}

impl<S: Store + Keyring> DynStore for S {
    fn holds(&self, held: Held) -> bool {
        match held {
            Held::Ledger => self.ledger().is_some(),
            Held::Cursors => self.cursors().is_some(),
            Held::Schedules => self.schedules().is_some(),
            Held::Subscriptions => self.subscriptions().is_some(),
            Held::Pins => self.pins().is_some(),
            Held::Deliveries => self.deliveries().is_some(),
            Held::Notes => self.notes().is_some(),
            Held::Latest => self.announcements().is_some(),
            Held::Inboxes => self.inboxes().is_some(),
            Held::Positions => self.positions().is_some(),
        }
    }

    fn skipped(&self) -> Vec<String> {
        Store::skipped(self)
    }

    fn writes_one_at_a_time(&self) -> bool {
        Store::writes_one_at_a_time(self)
    }

    fn ledger_keeping(&self) -> Keeping {
        Ledger::keeping(&self.ledger())
    }

    fn entry_boxed<'a>(
        &'a self,
        key: &'a Key,
    ) -> Boxed<'a, Result<Option<Versioned<Entry>>, StoreError>> {
        Box::pin(async move { self.ledger().entry(key).await })
    }

    fn put_entry_boxed<'a>(
        &'a self,
        entry: &'a Entry,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.ledger().put_entry(entry, expected).await })
    }

    fn expire_entries_boxed(&self, before: Timestamp) -> Boxed<'_, Result<Vec<Key>, StoreError>> {
        Box::pin(async move { self.ledger().expire_entries(before).await })
    }

    fn cursor_boxed<'a>(
        &'a self,
        source: &'a SourceName,
    ) -> Boxed<'a, Result<Option<Versioned<Cursor>>, StoreError>> {
        Box::pin(async move { self.cursors().cursor(source).await })
    }

    fn put_cursor_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        cursor: &'a Cursor,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.cursors().put_cursor(source, cursor, expected).await })
    }

    fn schedules_keeping(&self) -> Keeping {
        Schedules::keeping(&self.schedules())
    }

    fn every_schedule_boxed(&self) -> Boxed<'_, Result<Vec<Schedule>, StoreError>> {
        Box::pin(async move { self.schedules().every_schedule().await })
    }

    fn put_schedules_boxed<'a>(
        &'a self,
        add: &'a [Schedule],
        remove: &'a [Schedule],
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.schedules().put_schedules(add, remove).await })
    }

    fn subscribers_boxed<'a>(
        &'a self,
        destination: &'a DestinationName,
        project: &'a str,
    ) -> Boxed<'a, Result<Vec<Subscription>, StoreError>> {
        Box::pin(async move { self.subscriptions().subscribers(destination, project).await })
    }

    fn subscribe_boxed<'a>(
        &'a self,
        subscription: &'a Subscription,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.subscriptions().subscribe(subscription).await })
    }

    fn unsubscribe_boxed<'a>(
        &'a self,
        subscription: &'a Subscription,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.subscriptions().unsubscribe(subscription).await })
    }

    fn pinned_boxed<'a>(
        &'a self,
        route: &'a RouteId,
    ) -> Boxed<'a, Result<Option<String>, StoreError>> {
        Box::pin(async move { self.pins().pinned(route).await })
    }

    fn put_pinned_boxed<'a>(
        &'a self,
        route: &'a RouteId,
        post: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.pins().put_pinned(route, post).await })
    }

    fn position_boxed<'a>(
        &'a self,
        destination: &'a DestinationName,
    ) -> Boxed<'a, Result<Option<String>, StoreError>> {
        Box::pin(async move { self.positions().position(destination).await })
    }

    fn put_position_boxed<'a>(
        &'a self,
        destination: &'a DestinationName,
        position: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.positions().put_position(destination, position).await })
    }

    fn admit_boxed<'a>(
        &'a self,
        scheme: Scheme,
        id: &'a str,
        at: Timestamp,
    ) -> Boxed<'a, Result<Admitted, StoreError>> {
        Box::pin(async move { self.deliveries().admit(scheme, id, at).await })
    }

    fn release_boxed<'a>(
        &'a self,
        scheme: Scheme,
        id: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.deliveries().release(scheme, id).await })
    }

    fn expire_deliveries_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>> {
        Box::pin(async move { self.deliveries().expire_deliveries(before).await })
    }

    fn note_boxed<'a>(&'a self, key: &'a Key) -> Boxed<'a, Result<Option<String>, StoreError>> {
        Box::pin(async move { self.notes().note(key).await })
    }

    fn put_note_boxed<'a>(
        &'a self,
        actor: &'a str,
        key: &'a Key,
        note: &'a str,
        at: Timestamp,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.notes().put_note(actor, key, note, at).await })
    }

    fn outbox_boxed<'a>(
        &'a self,
        actor: &'a str,
        after: Option<&'a str>,
        most: usize,
    ) -> Boxed<'a, Result<Outboxed, StoreError>> {
        Box::pin(async move { self.notes().outbox(actor, after, most).await })
    }

    fn expire_notes_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>> {
        Box::pin(async move { self.notes().expire_notes(before).await })
    }

    fn latest_boxed<'a>(
        &'a self,
        project: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<Latest>>, StoreError>> {
        Box::pin(async move { self.announcements().latest(project).await })
    }

    fn every_latest_boxed(&self) -> Boxed<'_, Result<Vec<Latest>, StoreError>> {
        Box::pin(async move { self.announcements().every_latest().await })
    }

    fn put_latest_boxed<'a>(
        &'a self,
        latest: &'a Latest,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.announcements().put_latest(latest, expected).await })
    }

    fn delivery_boxed<'a>(
        &'a self,
        key: &'a Key,
        inbox: &'a str,
    ) -> Boxed<'a, Result<Option<State>, StoreError>> {
        Box::pin(async move { self.inboxes().delivery(key, inbox).await })
    }

    fn put_delivery_boxed<'a>(
        &'a self,
        key: &'a Key,
        inbox: &'a str,
        state: &'a State,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.inboxes().put_delivery(key, inbox, state).await })
    }

    fn forget_deliveries_boxed<'a>(&'a self, key: &'a Key) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.inboxes().forget_deliveries(key).await })
    }

    fn known_boxed<'a>(&'a self, id: &'a str) -> Boxed<'a, Result<Option<KnownKey>, StoreError>> {
        Box::pin(Keyring::known(self, id))
    }

    fn know_boxed<'a>(
        &'a self,
        id: &'a str,
        key: &'a KnownKey,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(Keyring::know(self, id, key))
    }

    fn forget_before_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>> {
        Box::pin(Keyring::forget_before(self, before))
    }
}

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

impl Deliveries for Erased<'_, dyn DynStore + '_> {
    async fn admit(&self, scheme: Scheme, id: &str, at: Timestamp) -> Result<Admitted, StoreError> {
        self.0.admit_boxed(scheme, id, at).await
    }

    async fn release(&self, scheme: Scheme, id: &str) -> Result<(), StoreError> {
        self.0.release_boxed(scheme, id).await
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

#[cfg(test)]
mod tests;
