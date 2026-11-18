//! A store that answers every call with the same error.

use herald_core::DestinationName;
use herald_ports::{Held, Keeper, StoreError};
use jiff::Timestamp;

use super::Refusing;

impl herald_ports::Inboxes for Refusing {
    async fn delivery(
        &self,
        _: &herald_core::Key,
        _: &str,
    ) -> Result<Option<herald_core::State>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_delivery(
        &self,
        _: &herald_core::Key,
        _: &str,
        _: &herald_core::State,
    ) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn forget_deliveries(&self, _: &herald_core::Key) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Ledger for Refusing {
    async fn entry(
        &self,
        _: &herald_core::Key,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Entry>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_entry(
        &self,
        _: &herald_core::Entry,
        _: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        Err(self.0.clone())
    }

    async fn expire_entries(&self, _: Timestamp) -> Result<Vec<herald_core::Key>, StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Cursors for Refusing {
    async fn cursor(
        &self,
        _: &herald_core::SourceName,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Cursor>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_cursor(
        &self,
        _: &herald_core::SourceName,
        _: &herald_core::Cursor,
        _: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        Err(self.0.clone())
    }

    async fn listing(
        &self,
        _: &herald_core::SourceName,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Listing>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_listing(
        &self,
        _: &herald_core::SourceName,
        _: &herald_core::Listing,
        _: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        Err(self.0.clone())
    }

    async fn members(
        &self,
        _: &herald_core::SourceName,
    ) -> Result<Vec<herald_ports::Versioned<herald_core::Membership>>, StoreError> {
        Err(self.0.clone())
    }

    async fn member(
        &self,
        _: &herald_core::SourceName,
        _: &str,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Membership>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_member(
        &self,
        _: &herald_core::SourceName,
        _: &herald_core::Membership,
        _: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        Err(self.0.clone())
    }

    async fn forget_member(&self, _: &herald_core::SourceName, _: &str) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Schedules for Refusing {
    async fn every_schedule(&self) -> Result<Vec<herald_core::Schedule>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_schedules(
        &self,
        _: &[herald_core::Schedule],
        _: &[herald_core::Schedule],
    ) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Subscriptions for Refusing {
    async fn subscribers(
        &self,
        _: &herald_core::DestinationName,
        _: &str,
    ) -> Result<Vec<herald_core::Subscription>, StoreError> {
        Err(self.0.clone())
    }

    async fn subscribe(&self, _: &herald_core::Subscription) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn unsubscribe(&self, _: &herald_core::Subscription) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Pins for Refusing {
    async fn pinned(&self, _: &herald_core::RouteId) -> Result<Option<String>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_pinned(&self, _: &herald_core::RouteId, _: &str) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn forget_pinned(&self, _: &herald_core::RouteId, _: &str) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Decisions for Refusing {
    async fn decision(
        &self,
        _: &herald_core::Key,
    ) -> Result<Option<herald_core::Decided>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_decision(&self, _: &herald_core::Decided) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn expire_decisions(&self, _: jiff::Timestamp) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Positions for Refusing {
    async fn position(&self, _: &DestinationName) -> Result<Option<String>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_position(&self, _: &DestinationName, _: &str) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Deliveries for Refusing {
    async fn admit(
        &self,
        _: &'static str,
        _: &str,
        _: Timestamp,
    ) -> Result<herald_ports::Admitted, StoreError> {
        Err(self.0.clone())
    }

    async fn seen(&self, _: &'static str, _: &str) -> Result<bool, StoreError> {
        Err(self.0.clone())
    }

    async fn release(&self, _: &'static str, _: &str) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn expire_deliveries(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Notes for Refusing {
    async fn note(&self, _: &herald_core::Key) -> Result<Option<String>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_note(
        &self,
        _: &str,
        _: &herald_core::Key,
        _: &str,
        _: Timestamp,
    ) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn outbox(
        &self,
        _: &str,
        _: Option<&str>,
        _: usize,
    ) -> Result<herald_ports::Outboxed, StoreError> {
        Err(self.0.clone())
    }

    async fn expire_notes(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn forget_note(&self, _: &herald_core::Key) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Announcements for Refusing {
    async fn latest(
        &self,
        _: &str,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Latest>>, StoreError> {
        Err(self.0.clone())
    }

    async fn every_latest(&self) -> Result<Vec<herald_core::Latest>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_latest(
        &self,
        _: &herald_core::Latest,
        _: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        Err(self.0.clone())
    }

    async fn forget_latest(&self, _: &str) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl Keeper for Refusing {
    fn holds(&self, _: Held) -> bool {
        true
    }

    fn skipped(&self) -> Vec<String> {
        Vec::new()
    }
}
