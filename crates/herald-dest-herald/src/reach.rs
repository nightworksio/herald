use std::future::ready;

use herald_core::{
    Announcement, Bullet, Chore, Delivery, Emphases, Entry, Escape, Features, Fences, Heading,
    Limits, Links, Markup, Measure, MentionStyle, Pins, Profile, Sent, Step, Symbols, Threads,
};
use herald_kinds::{Fed, Feeding, Reach, Signing, UnreadPosts};
use herald_ports::{Base, Boxed, DynHttp, DynMarkers, HttpError, Secret};

use crate::FeedOptions;

/// Why a feed reads nothing back: herald serves its envelopes rather than
/// posting them somewhere it could read.
pub(crate) const UNREAD: &str = "a herald feed reads no posts back; herald serves its envelopes";

/// Why a feed is sent nothing: herald publishes each event to it itself.
const UNSENT: &str = "a herald feed is published to by herald itself, not sent to";

/// The most bytes herald counts in anything a feed shows: the most an
/// envelope may take.
const SHOWN: usize = FeedOptions::ENVELOPE_ALLOWED;

/// A feed of herald's own, reached: what it lists, and the instance's key
/// its envelopes are proved by.
#[derive(Debug, Clone, PartialEq)]
pub struct Feed {
    feeding: Feeding,
    key: Secret,
}

impl Feed {
    /// A feed listing as `feeding` says, its envelopes proved by `key`.
    #[must_use]
    pub const fn new(feeding: Feeding, key: Secret) -> Self {
        Self { feeding, key }
    }
}

/// What a feed can do: it carries each event as an envelope, edits by listing the
/// edited event again, and reads nothing back.
pub(crate) const fn profile() -> Profile {
    Profile {
        markup: Markup {
            escape: Escape::Neutral,
            links: Links::Spelled,
            heading: Heading::Bare,
            marks: Emphases {
                emphasis: "",
                strong: "",
                strike: "",
            },
            fences: Fences::None,
            bullet: Bullet::Symbol('-'),
            html: false,
            pills: None,
        },
        measure: Measure::Bytes,
        limits: Limits {
            title: SHOWN,
            text: SHOWN,
            followup: SHOWN,
            thread_name: SHOWN,
        },
        features: Features {
            threads: Threads::None,
            crosspost: false,
            pins: Pins::None,
            reads_back: false,
            edits: true,
            mentions: MentionStyle::None,
            interactions: false,
        },
        symbols: Symbols::DEFAULT,
    }
}

impl Reach for Feed {
    fn profile(&self) -> Profile {
        profile()
    }

    fn base(&self) -> Base<'_> {
        Base::Among(&[])
    }

    fn send<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Signing<'a>,
        _: &'a Step,
        _: &'a Announcement,
        _: &'a Delivery,
        _: &'a Entry,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(ready(Ok(Sent::Refused(UNSENT.to_owned()))))
    }

    fn housekeep<'a>(
        &'a self,
        _: &'a dyn DynHttp,
        _: Chore,
        _: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(ready(Ok(Sent::Refused(UNSENT.to_owned()))))
    }

    fn posts<'a>(&'a self, _: &'a dyn DynHttp) -> Box<dyn DynMarkers + 'a> {
        Box::new(UnreadPosts(UNREAD))
    }

    fn feeds(&self) -> Option<Fed<'_>> {
        Some(Fed {
            feeding: self.feeding,
            key: &self.key,
        })
    }
}
