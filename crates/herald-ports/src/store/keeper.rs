//! A store that holds every kind of record it keeps itself, as one value.

use super::{
    Announcements, Cursors, Decisions, Deliveries, Held, Inboxes, Ledger, Notes, Pins, Positions,
    Published, Pulls, Recalls, Schedules, Store, Subscriptions, Tokens, Waiting,
};

/// A store holding every kind of record it keeps itself, and saying which
/// kinds it holds.
pub trait Keeper:
    Ledger
    + Cursors
    + Schedules
    + Subscriptions
    + Pins
    + Deliveries
    + Notes
    + Announcements
    + Inboxes
    + Positions
    + Decisions
    + Pulls
    + Recalls
    + Waiting
    + Tokens
    + Published
{
    /// Whether it holds records of `kind`.
    fn holds(&self, kind: Held) -> bool;

    /// Whether a run writes one record at a time: where a write may be
    /// refused because another record was written at once, or where writes
    /// are not compared and set.
    fn writes_one_at_a_time(&self) -> bool {
        false
    }

    /// The paths of the records a listing skipped as corrupt since this was
    /// last asked, each once.
    fn skipped(&self) -> Vec<String>;
}

impl<K: Keeper> Store for K {
    type Ledger = Self;
    type Cursors = Self;
    type Schedules = Self;
    type Subscriptions = Self;
    type Pins = Self;
    type Deliveries = Self;
    type Notes = Self;
    type Announcements = Self;
    type Inboxes = Self;
    type Positions = Self;
    type Waiting = Self;
    type Tokens = Self;
    type Published = Self;
    type Decisions = Self;
    type Pulls = Self;
    type Recalls = Self;

    fn ledger(&self) -> Option<&Self> {
        self.holds(Held::Ledger).then_some(self)
    }

    fn cursors(&self) -> Option<&Self> {
        self.holds(Held::Cursors).then_some(self)
    }

    fn schedules(&self) -> Option<&Self> {
        self.holds(Held::Schedules).then_some(self)
    }

    fn subscriptions(&self) -> Option<&Self> {
        self.holds(Held::Subscriptions).then_some(self)
    }

    fn pins(&self) -> Option<&Self> {
        self.holds(Held::Pins).then_some(self)
    }

    fn deliveries(&self) -> Option<&Self> {
        self.holds(Held::Deliveries).then_some(self)
    }

    fn notes(&self) -> Option<&Self> {
        self.holds(Held::Notes).then_some(self)
    }

    fn announcements(&self) -> Option<&Self> {
        self.holds(Held::Latest).then_some(self)
    }

    fn inboxes(&self) -> Option<&Self> {
        self.holds(Held::Inboxes).then_some(self)
    }

    fn positions(&self) -> Option<&Self> {
        self.holds(Held::Positions).then_some(self)
    }

    fn decisions(&self) -> Option<&Self> {
        self.holds(Held::Decisions).then_some(self)
    }

    fn pulls(&self) -> Option<&Self> {
        self.holds(Held::Pulls).then_some(self)
    }

    fn recalls(&self) -> Option<&Self> {
        self.holds(Held::Recalls).then_some(self)
    }

    fn waiting(&self) -> Option<&Self> {
        self.holds(Held::Pending).then_some(self)
    }

    fn tokens(&self) -> Option<&Self> {
        self.holds(Held::Tokens).then_some(self)
    }

    fn published(&self) -> Option<&Self> {
        self.holds(Held::Published).then_some(self)
    }

    fn skipped(&self) -> Vec<String> {
        Keeper::skipped(self)
    }

    fn writes_one_at_a_time(&self) -> bool {
        Keeper::writes_one_at_a_time(self)
    }
}
