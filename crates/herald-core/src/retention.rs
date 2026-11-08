use jiff::SignedDuration;

/// How long a store keeps what herald records before housekeeping forgets
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    /// How long a webhook delivery's id is kept after it was admitted
    /// (ADR-0013).
    pub deliveries: SignedDuration,
    /// How long a ledger entry, and its deliveries to followers' inboxes,
    /// are kept after a run last claimed it; for ever where `None`.
    pub ledger: Option<SignedDuration>,
    /// How long a Note is kept after it was published; for ever where
    /// `None`.
    pub notes: Option<SignedDuration>,
}

impl Retention {
    /// How long a delivery's id is kept by default: a week.
    pub const DELIVERIES: SignedDuration = SignedDuration::from_hours(7 * 24);
    /// How long a ledger entry is kept by default: 90 days.
    pub const LEDGER: SignedDuration = SignedDuration::from_hours(90 * 24);
    /// The shortest a delivery's id may be kept: longer than any sender
    /// herald admits from goes on retrying one. Mastodon's sixteen retries
    /// span about two days.
    pub const LONGEST_RETRY: SignedDuration = SignedDuration::from_hours(3 * 24);
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            deliveries: Self::DELIVERIES,
            ledger: Some(Self::LEDGER),
            notes: None,
        }
    }
}
