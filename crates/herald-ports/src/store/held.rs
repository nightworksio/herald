//! The kinds of record a store may hold.

use super::StoreError;

/// A kind of record a store may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    /// The ledger.
    Ledger,
    /// Cursors.
    Cursors,
    /// Schedules.
    Schedules,
    /// Subscriptions.
    Subscriptions,
    /// Each route's pinned post.
    Pins,
    /// The ids of webhook deliveries it admitted.
    Deliveries,
    /// The Notes herald's ActivityPub actors published.
    Notes,
    /// What each project last announced.
    Latest,
    /// Where each delivery to a follower's inbox stands.
    Inboxes,
    /// Where each session a destination holds stands.
    Positions,
    /// Events held back until they are due.
    Pending,
    /// Minted tokens, sealed.
    Tokens,
    /// What the instance published to its peers.
    Published,
    /// What models answered the tasks views asked (ADR-0057).
    Decisions,
    /// The pull requests kept mode keeps (ADR-0061).
    Pulls,
    /// The reports whose living posts were sent advisories (ADR-0067).
    Recalls,
    /// The record of each re-run an operator applied, of each gesture that
    /// queued a pull request and of each pull request's last leaving of
    /// herald's own queue (ADR-0068, ADR-0087).
    Actions,
}

impl Held {
    /// Every kind, in the order a store's accessors list them.
    pub const ALL: [Self; 17] = [
        Self::Ledger,
        Self::Cursors,
        Self::Schedules,
        Self::Subscriptions,
        Self::Pins,
        Self::Deliveries,
        Self::Notes,
        Self::Latest,
        Self::Inboxes,
        Self::Positions,
        Self::Pending,
        Self::Tokens,
        Self::Published,
        Self::Decisions,
        Self::Pulls,
        Self::Recalls,
        Self::Actions,
    ];

    /// The kind as an error names it.
    const fn what(self) -> &'static str {
        match self {
            Self::Ledger => "ledger",
            Self::Cursors => "cursors",
            Self::Schedules => "schedules",
            Self::Subscriptions => "subscriptions",
            Self::Pins => "pins",
            Self::Deliveries => "deliveries",
            Self::Notes => "notes",
            Self::Latest => "latest",
            Self::Inboxes => "deliveries to followers",
            Self::Positions => "session positions",
            Self::Pending => "held events",
            Self::Tokens => "minted tokens",
            Self::Published => "published key sets, feeds, and peers' key sets and audits",
            Self::Decisions => "decisions",
            Self::Pulls => "pull requests",
            Self::Recalls => "reports whose living posts were sent advisories",
            Self::Actions => "re-runs",
        }
    }

    /// The error a store keeping none of this kind answers.
    #[must_use]
    pub const fn unsupported(self) -> StoreError {
        StoreError::Unsupported { what: self.what() }
    }
}
