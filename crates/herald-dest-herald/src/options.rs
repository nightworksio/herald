use std::num::NonZeroUsize;

use herald_core::{Profile, SecretRef, Size, Symbols, WaitOptions};
use herald_kinds::{
    Bound, Destination, DestinationConfig, Fault, Feeding, Placed, Placement, Reached,
};
use herald_peers::feed;
use herald_ports::Secret;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::HERALD;
use crate::reach::{Feed, profile};

/// What a feed of herald's own keeps and lists, as `[destinations.*]`
/// writes it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedOptions {
    /// The most envelopes the feed keeps, newest first: 1000 where unset,
    /// fewer where the store's records hold fewer, and at most 4000.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub keep: Option<NonZeroUsize>,
    /// The most envelopes a page lists: 20 where unset, and at most 80.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub page: Option<NonZeroUsize>,
    /// The most bytes one envelope takes, its proof counted: "16KiB" where
    /// unset, and at most "64KiB". A longer event's body is cut to fit.
    pub envelope: Option<Size>,
    /// How many envelopes the store's records hold, where they hold fewer
    /// than the default and `keep` is unset.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) fitted: Option<usize>,
}

impl FeedOptions {
    /// The envelopes a feed keeps where `keep` is unset.
    pub const KEEP: usize = 1000;
    /// The most envelopes a feed keeps.
    pub const KEEP_ALLOWED: usize = 4 * Self::KEEP;
    /// The envelopes a page lists where `page` is unset.
    pub const PAGE: usize = feed::PAGE;
    /// The most envelopes a page lists.
    pub const PAGE_ALLOWED: usize = feed::PAGE_MOST;
    /// The bytes an envelope takes at most where `envelope` is unset.
    pub const ENVELOPE: usize = feed::ENVELOPE;
    /// The most bytes `envelope` allows.
    pub const ENVELOPE_ALLOWED: usize = feed::ENVELOPE_MOST;

    /// The options, where every one is within its bound.
    ///
    /// # Errors
    ///
    /// The first option past its bound.
    pub fn check(self) -> Result<Self, Fault> {
        let over = [
            ("keep", self.keep.map(NonZeroUsize::get), Self::KEEP_ALLOWED),
            ("page", self.page.map(NonZeroUsize::get), Self::PAGE_ALLOWED),
        ]
        .into_iter()
        .find(|(_, set, most)| set.is_some_and(|set| set > *most));
        if let Some((key, _, most)) = over {
            return Err(Fault::at(key, format!("{key} is at most {most}; lower it")));
        }
        if self
            .envelope
            .is_some_and(|envelope| envelope.0 > Self::ENVELOPE_ALLOWED)
        {
            return Err(Fault::at(
                "envelope",
                format!(
                    "envelope is at most {}; lower it",
                    Size::written(Self::ENVELOPE_ALLOWED)
                ),
            ));
        }
        Ok(self)
    }
}

/// What a route says about a feed of herald's own alone: nothing yet.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedRouteOptions {}

impl Placement for FeedRouteOptions {}

impl DestinationConfig for FeedOptions {
    fn kind(&self) -> &'static str {
        HERALD
    }

    fn symbols_written(&self) -> Option<Symbols> {
        None
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        None
    }

    fn credential(&self) -> Option<&SecretRef> {
        None
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        Vec::new()
    }

    fn unread_why(&self) -> &'static str {
        crate::reach::UNREAD
    }

    fn feeding(&self) -> Option<Feeding> {
        let (keep, bound) = match (self.keep, self.fitted) {
            (Some(keep), _) => (keep.get(), Bound::Named),
            (None, Some(fitted)) => (fitted, Bound::Fitted),
            (None, None) => (Self::KEEP, Bound::Default),
        };
        Some(Feeding {
            keep,
            page: self.page.map_or(Self::PAGE, NonZeroUsize::get),
            envelope: self.envelope.map_or(Self::ENVELOPE, |envelope| envelope.0),
            bound,
        })
    }

    fn fitted(&self, keep: usize) -> Option<Destination> {
        self.keep.is_none().then(|| {
            Destination::new(Self {
                fitted: Some(keep),
                ..self.clone()
            })
        })
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        placed.of::<FeedRouteOptions>()?;
        Some((profile(), None))
    }

    fn reach(&self, placed: &Placed, _: &str, secret: Secret) -> Option<Reached> {
        placed.of::<FeedRouteOptions>()?;
        let feeding = self.feeding()?;
        Some(Reached::new(Feed::new(feeding, secret)))
    }
}
