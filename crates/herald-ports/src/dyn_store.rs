use crate::{Decisions, Pulls};
use herald_core::{
    AuditDay, Cursor, DestinationName, Entry, Feed, Hold, KeptToken, Key, Latest, Listing,
    Membership, PeerKeys, Pending, Publication, PublishedKeys, RouteId, Schedule, SourceName,
    State, Subscription, Window,
};
use herald_core::{Decided, KeptPull, PullKey, Reconciled, RepositoryKey};
use jiff::Timestamp;

use crate::erased::Boxed;
use crate::keyring::{Keyring, KnownKey};
use crate::store::{
    Admitted, Announcements, Cursors, Deliveries, Held, Inboxes, Keeping, Ledger, Notes, Outboxed,
    Pins, Positions, Published, Schedules, Store, StoreError, Subscriptions, Tokens, Version,
    Versioned, Waiting, Written,
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

    /// [`Cursors::listing`].
    fn listing_boxed<'a>(
        &'a self,
        source: &'a SourceName,
    ) -> Boxed<'a, Result<Option<Versioned<Listing>>, StoreError>>;

    /// [`Cursors::put_listing`].
    fn put_listing_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        listing: &'a Listing,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Cursors::members`].
    fn members_boxed<'a>(
        &'a self,
        source: &'a SourceName,
    ) -> Boxed<'a, Result<Vec<Versioned<Membership>>, StoreError>>;

    /// [`Cursors::member`].
    fn member_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        id: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<Membership>>, StoreError>>;

    /// [`Cursors::put_member`].
    fn put_member_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        membership: &'a Membership,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Cursors::forget_member`].
    fn forget_member_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        id: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>>;

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

    /// [`Pins::forget_pinned`].
    fn forget_pinned_boxed<'a>(
        &'a self,
        route: &'a RouteId,
        post: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Decisions::decision`].
    fn decision_boxed<'a>(&'a self, key: &'a Key)
    -> Boxed<'a, Result<Option<Decided>, StoreError>>;

    /// [`Decisions::put_decision`].
    fn put_decision_boxed<'a>(&'a self, decided: &'a Decided) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Decisions::expire_decisions`].
    fn expire_decisions_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>>;

    /// [`Pulls::pull`].
    fn pull_boxed<'a>(
        &'a self,
        key: &'a PullKey,
    ) -> Boxed<'a, Result<Option<Versioned<KeptPull>>, StoreError>>;

    /// [`Pulls::pulls_of`].
    fn pulls_of_boxed<'a>(
        &'a self,
        repository: &'a RepositoryKey,
    ) -> Boxed<'a, Result<Vec<Versioned<KeptPull>>, StoreError>>;

    /// [`Pulls::put_pull`].
    fn put_pull_boxed<'a>(
        &'a self,
        kept: &'a KeptPull,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Pulls::expire_pulls`].
    fn expire_pulls_of_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>>;

    /// [`Pulls::reconciled`].
    fn reconciled_boxed<'a>(
        &'a self,
        source: &'a SourceName,
    ) -> Boxed<'a, Result<Option<Versioned<Reconciled>>, StoreError>>;

    /// [`Pulls::put_reconciled`].
    fn put_reconciled_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        reconciled: &'a Reconciled,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

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

    /// [`Waiting::pending`].
    fn pending_boxed<'a>(
        &'a self,
        hold: &'a Hold,
    ) -> Boxed<'a, Result<Option<Versioned<Pending>>, StoreError>>;

    /// [`Waiting::every_pending`].
    fn every_pending_boxed(&self) -> Boxed<'_, Result<Vec<Versioned<Pending>>, StoreError>>;

    /// [`Waiting::put_pending`].
    fn put_pending_boxed<'a>(
        &'a self,
        pending: &'a Pending,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Waiting::drop_pending`].
    fn drop_pending_boxed<'a>(&'a self, hold: &'a Hold) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Tokens::token`].
    fn token_boxed<'a>(
        &'a self,
        name: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<KeptToken>>, StoreError>>;

    /// [`Tokens::put_token`].
    fn put_token_boxed<'a>(
        &'a self,
        token: &'a KeptToken,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Published::published_keys`].
    fn published_keys_boxed(
        &self,
    ) -> Boxed<'_, Result<Option<Versioned<PublishedKeys>>, StoreError>>;

    /// [`Published::put_published_keys`].
    fn put_published_keys_boxed<'a>(
        &'a self,
        keys: &'a PublishedKeys,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Published::publication`].
    fn publication_boxed(&self) -> Boxed<'_, Result<Option<Versioned<Publication>>, StoreError>>;

    /// [`Published::put_publication`].
    fn put_publication_boxed<'a>(
        &'a self,
        publication: &'a Publication,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Published::feed`].
    fn feed_boxed<'a>(
        &'a self,
        destination: &'a DestinationName,
    ) -> Boxed<'a, Result<Option<Versioned<Feed>>, StoreError>>;

    /// [`Published::put_feed`].
    fn put_feed_boxed<'a>(
        &'a self,
        destination: &'a DestinationName,
        feed: &'a Feed,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Published::peer_keys`].
    fn peer_keys_boxed<'a>(
        &'a self,
        peer: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<PeerKeys>>, StoreError>>;

    /// [`Published::put_peer_keys`].
    fn put_peer_keys_boxed<'a>(
        &'a self,
        peer: &'a str,
        keys: &'a PeerKeys,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Published::audit`].
    fn audit_boxed<'a>(
        &'a self,
        peer: &'a str,
        day: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<AuditDay>>, StoreError>>;

    /// [`Published::put_audit`].
    fn put_audit_boxed<'a>(
        &'a self,
        audit: &'a AuditDay,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Published::audits`].
    fn audits_boxed<'a>(&'a self, peer: &'a str) -> Boxed<'a, Result<Vec<AuditDay>, StoreError>>;

    /// [`Published::expire_audits`].
    fn expire_audits_boxed<'a>(&'a self, day: &'a str) -> Boxed<'a, Result<(), StoreError>>;

    /// [`Published::window`].
    fn window_boxed<'a>(
        &'a self,
        peer: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<Window>>, StoreError>>;

    /// [`Published::put_window`].
    fn put_window_boxed<'a>(
        &'a self,
        peer: &'a str,
        window: &'a Window,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Deliveries::admit`].
    fn admit_boxed<'a>(
        &'a self,
        namespace: &'static str,
        id: &'a str,
        at: Timestamp,
    ) -> Boxed<'a, Result<Admitted, StoreError>>;

    /// [`Deliveries::seen`].
    fn seen_boxed<'a>(
        &'a self,
        namespace: &'static str,
        id: &'a str,
    ) -> Boxed<'a, Result<bool, StoreError>>;

    /// [`Deliveries::release`].
    fn release_boxed<'a>(
        &'a self,
        namespace: &'static str,
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

    /// [`Notes::forget_note`].
    fn forget_note_boxed<'a>(&'a self, key: &'a Key) -> Boxed<'a, Result<(), StoreError>>;

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

    /// [`Announcements::forget_latest`].
    fn forget_latest_boxed<'a>(&'a self, project: &'a str) -> Boxed<'a, Result<(), StoreError>>;

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
            Held::Pending => self.waiting().is_some(),
            Held::Tokens => self.tokens().is_some(),
            Held::Published => self.published().is_some(),
            Held::Decisions => self.decisions().is_some(),
            Held::Pulls => self.pulls().is_some(),
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

    fn listing_boxed<'a>(
        &'a self,
        source: &'a SourceName,
    ) -> Boxed<'a, Result<Option<Versioned<Listing>>, StoreError>> {
        Box::pin(async move { self.cursors().listing(source).await })
    }

    fn put_listing_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        listing: &'a Listing,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.cursors().put_listing(source, listing, expected).await })
    }

    fn members_boxed<'a>(
        &'a self,
        source: &'a SourceName,
    ) -> Boxed<'a, Result<Vec<Versioned<Membership>>, StoreError>> {
        Box::pin(async move { self.cursors().members(source).await })
    }

    fn member_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        id: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<Membership>>, StoreError>> {
        Box::pin(async move { self.cursors().member(source, id).await })
    }

    fn put_member_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        membership: &'a Membership,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move {
            self.cursors()
                .put_member(source, membership, expected)
                .await
        })
    }

    fn forget_member_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        id: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.cursors().forget_member(source, id).await })
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

    fn forget_pinned_boxed<'a>(
        &'a self,
        route: &'a RouteId,
        post: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.pins().forget_pinned(route, post).await })
    }

    fn decision_boxed<'a>(
        &'a self,
        key: &'a Key,
    ) -> Boxed<'a, Result<Option<Decided>, StoreError>> {
        Box::pin(async move { self.decisions().decision(key).await })
    }

    fn put_decision_boxed<'a>(&'a self, decided: &'a Decided) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.decisions().put_decision(decided).await })
    }

    fn expire_decisions_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>> {
        Box::pin(async move { self.decisions().expire_decisions(before).await })
    }

    fn pull_boxed<'a>(
        &'a self,
        key: &'a PullKey,
    ) -> Boxed<'a, Result<Option<Versioned<KeptPull>>, StoreError>> {
        Box::pin(async move { self.pulls().pull(key).await })
    }

    fn pulls_of_boxed<'a>(
        &'a self,
        repository: &'a RepositoryKey,
    ) -> Boxed<'a, Result<Vec<Versioned<KeptPull>>, StoreError>> {
        Box::pin(async move { self.pulls().pulls_of(repository).await })
    }

    fn put_pull_boxed<'a>(
        &'a self,
        kept: &'a KeptPull,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.pulls().put_pull(kept, expected).await })
    }

    fn expire_pulls_of_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>> {
        Box::pin(async move { self.pulls().expire_pulls(before).await })
    }

    fn reconciled_boxed<'a>(
        &'a self,
        source: &'a SourceName,
    ) -> Boxed<'a, Result<Option<Versioned<Reconciled>>, StoreError>> {
        Box::pin(async move { self.pulls().reconciled(source).await })
    }

    fn put_reconciled_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        reconciled: &'a Reconciled,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move {
            self.pulls()
                .put_reconciled(source, reconciled, expected)
                .await
        })
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

    fn pending_boxed<'a>(
        &'a self,
        hold: &'a Hold,
    ) -> Boxed<'a, Result<Option<Versioned<Pending>>, StoreError>> {
        Box::pin(async move { self.waiting().pending(hold).await })
    }

    fn every_pending_boxed(&self) -> Boxed<'_, Result<Vec<Versioned<Pending>>, StoreError>> {
        Box::pin(async move { self.waiting().every_pending().await })
    }

    fn put_pending_boxed<'a>(
        &'a self,
        pending: &'a Pending,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.waiting().put_pending(pending, expected).await })
    }

    fn drop_pending_boxed<'a>(&'a self, hold: &'a Hold) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.waiting().drop_pending(hold).await })
    }

    fn token_boxed<'a>(
        &'a self,
        name: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<KeptToken>>, StoreError>> {
        Box::pin(async move { self.tokens().token(name).await })
    }

    fn put_token_boxed<'a>(
        &'a self,
        token: &'a KeptToken,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.tokens().put_token(token, expected).await })
    }

    fn published_keys_boxed(
        &self,
    ) -> Boxed<'_, Result<Option<Versioned<PublishedKeys>>, StoreError>> {
        Box::pin(async move { self.published().published_keys().await })
    }

    fn put_published_keys_boxed<'a>(
        &'a self,
        keys: &'a PublishedKeys,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.published().put_published_keys(keys, expected).await })
    }

    fn publication_boxed(&self) -> Boxed<'_, Result<Option<Versioned<Publication>>, StoreError>> {
        Box::pin(async move { self.published().publication().await })
    }

    fn put_publication_boxed<'a>(
        &'a self,
        publication: &'a Publication,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move {
            self.published()
                .put_publication(publication, expected)
                .await
        })
    }

    fn feed_boxed<'a>(
        &'a self,
        destination: &'a DestinationName,
    ) -> Boxed<'a, Result<Option<Versioned<Feed>>, StoreError>> {
        Box::pin(async move { self.published().feed(destination).await })
    }

    fn put_feed_boxed<'a>(
        &'a self,
        destination: &'a DestinationName,
        feed: &'a Feed,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.published().put_feed(destination, feed, expected).await })
    }

    fn peer_keys_boxed<'a>(
        &'a self,
        peer: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<PeerKeys>>, StoreError>> {
        Box::pin(async move { self.published().peer_keys(peer).await })
    }

    fn put_peer_keys_boxed<'a>(
        &'a self,
        peer: &'a str,
        keys: &'a PeerKeys,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.published().put_peer_keys(peer, keys, expected).await })
    }

    fn audit_boxed<'a>(
        &'a self,
        peer: &'a str,
        day: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<AuditDay>>, StoreError>> {
        Box::pin(async move { self.published().audit(peer, day).await })
    }

    fn put_audit_boxed<'a>(
        &'a self,
        audit: &'a AuditDay,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.published().put_audit(audit, expected).await })
    }

    fn audits_boxed<'a>(&'a self, peer: &'a str) -> Boxed<'a, Result<Vec<AuditDay>, StoreError>> {
        Box::pin(async move { self.published().audits(peer).await })
    }

    fn expire_audits_boxed<'a>(&'a self, day: &'a str) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.published().expire_audits(day).await })
    }

    fn window_boxed<'a>(
        &'a self,
        peer: &'a str,
    ) -> Boxed<'a, Result<Option<Versioned<Window>>, StoreError>> {
        Box::pin(async move { self.published().window(peer).await })
    }

    fn put_window_boxed<'a>(
        &'a self,
        peer: &'a str,
        window: &'a Window,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.published().put_window(peer, window, expected).await })
    }

    fn admit_boxed<'a>(
        &'a self,
        namespace: &'static str,
        id: &'a str,
        at: Timestamp,
    ) -> Boxed<'a, Result<Admitted, StoreError>> {
        Box::pin(async move { self.deliveries().admit(namespace, id, at).await })
    }

    fn seen_boxed<'a>(
        &'a self,
        namespace: &'static str,
        id: &'a str,
    ) -> Boxed<'a, Result<bool, StoreError>> {
        Box::pin(async move { self.deliveries().seen(namespace, id).await })
    }

    fn release_boxed<'a>(
        &'a self,
        namespace: &'static str,
        id: &'a str,
    ) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.deliveries().release(namespace, id).await })
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

    fn forget_note_boxed<'a>(&'a self, key: &'a Key) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.notes().forget_note(key).await })
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

    fn forget_latest_boxed<'a>(&'a self, project: &'a str) -> Boxed<'a, Result<(), StoreError>> {
        Box::pin(async move { self.announcements().forget_latest(project).await })
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

mod erased;
mod pulls;

mod published;

#[cfg(test)]
mod tests;
