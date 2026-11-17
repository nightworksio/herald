use herald_ports::Secret;

use crate::Bound;

/// What a destination that publishes to a feed of herald's own lists: how
/// many envelopes it keeps, how many a page lists, and the most bytes one
/// takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feeding {
    /// The most envelopes the feed keeps, newest first.
    pub keep: usize,
    /// The most envelopes a page lists.
    pub page: usize,
    /// The most bytes one envelope takes, its proof counted.
    pub envelope: usize,
    /// What chose `keep`.
    pub bound: Bound,
}

impl Feeding {
    /// The most bytes a feed record keeping its envelopes takes, what each
    /// envelope's entry holds beside it counted as [`Feeding::ITEM`] and
    /// the rest as [`Feeding::REST`].
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.keep
            .saturating_mul(self.envelope.saturating_add(Self::ITEM))
            .saturating_add(Self::REST)
    }

    /// Whether a feed record keeping its envelopes fits in `most` bytes.
    #[must_use]
    pub const fn fits(&self, most: usize) -> bool {
        self.bytes() <= most
    }

    /// The most envelopes whose feed record fits in `most` bytes.
    #[must_use]
    pub const fn fitting(&self, most: usize) -> usize {
        most.saturating_sub(Self::REST) / self.envelope.saturating_add(Self::ITEM)
    }

    /// The most bytes an envelope's entry holds beside it: its sequence and
    /// the names around them.
    pub const ITEM: usize = 64;

    /// The most bytes a feed record holds beside its entries: its newest
    /// sequence, and the record's own key and seal.
    pub const REST: usize = 4096;
}

/// A route's feed, as the flows publish to it: what it lists, and the
/// instance's key its envelopes are proved by.
#[derive(Debug, Clone, Copy)]
pub struct Fed<'a> {
    /// What it lists.
    pub feeding: Feeding,
    /// The key that proves each envelope: the instance's newest.
    pub key: &'a Secret,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feeding(keep: usize, envelope: usize) -> Feeding {
        Feeding {
            keep,
            page: 20,
            envelope,
            bound: Bound::Default,
        }
    }

    #[test]
    fn a_feed_record_holds_each_envelope_with_its_entry_and_the_rest() {
        let fed = feeding(1000, 16_384);
        assert_eq!(fed.bytes(), 1000 * (16_384 + 64) + 4096);
        assert!(fed.fits(fed.bytes()));
        assert!(!fed.fits(fed.bytes() - 1));
        assert_eq!(fed.fitting(65_536), 3);
        assert_eq!(fed.fitting(4096 + 2 * (16_384 + 64)), 2);
        assert_eq!(fed.fitting(4095 + 2 * (16_384 + 64)), 1);
        assert_eq!(fed.fitting(100), 0);
        assert_eq!(feeding(usize::MAX, 8).bytes(), usize::MAX);
    }
}
