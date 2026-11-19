use std::future::ready;

use herald_core::{
    Announcement, Chore, Delivery, Entry, Profile, SecretRef, Sent, Size, Step, Symbols,
    WaitOptions, Waits,
};
use herald_kinds::{
    Destination, DestinationConfig, DestinationKind, Fault, Peered, Placed, Placement, Placing,
    Pushed, Pushing, Reach, Reached, Signing, UnreadPosts,
};
use herald_peers::discovery::inbox_of;
use herald_peers::feed;
use herald_ports::{Base, Boxed, DynHttp, DynMarkers, HttpError, Secret};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::reach::profile;

/// The kind of a push to a peer, as the config spells it.
pub const HERALD_PUSH: &str = "herald-push";

/// Why a push reads nothing back: herald pushes its envelopes rather than
/// posting them somewhere it could read.
const UNREAD: &str = "a push to a peer reads no posts back; herald pushes its envelopes";

/// Why a push is sent nothing step by step: herald pushes each event
/// itself.
const UNSENT: &str = "a push to a peer is pushed to by herald itself, not sent to";

/// A push of each event to a peer's herald source, as a kind of
/// destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushKind {}

impl DestinationKind for PushKind {
    const NAME: &str = HERALD_PUSH;
    const DESCRIPTION: &str = "A peer's herald source each event is pushed to: an envelope proved by the instance's newest key, in a request signed by it.";
    const LABEL: &str = "herald push";
    const ROUTE: &str = "HeraldPushRoute";
    const ROUTE_DESCRIPTION: &str = "A route pushing its events to a peer's herald source.";

    type Options = PushOptions;
    type Route = PushRouteOptions;

    fn checked(
        options: PushOptions,
        _read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<(PushOptions, Vec<Fault>), Fault> {
        options.check().map(|options| (options, Vec::new()))
    }

    fn placed(_: &PushOptions, _: &PushRouteOptions, _: Placing<'_>) -> Vec<Fault> {
        Vec::new()
    }
}

/// Where a push to a peer goes, as `[destinations.*]` writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PushOptions {
    /// The peer of `[peers]` each event is pushed to.
    #[schemars(example = &"herald-example-net")]
    pub peer: String,
    /// The peer's herald source taking the pushes, whose inbox is
    /// `/herald/v1/inbox/<inbox>` under the peer's URL.
    #[schemars(example = &"releases")]
    pub inbox: String,
    /// The most bytes one envelope takes, its proof counted: "16KiB" where
    /// unset, and at most "64KiB". A longer event's body is cut to fit.
    pub envelope: Option<Size>,
    /// How long it waits before pushing again where the peer's inbox
    /// answers that it is unwell or limited and names no wait.
    pub waits: Option<WaitOptions>,
    /// The peer, as `[peers]` has it, once the config joins it.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) peered: Option<Peered>,
}

impl PushOptions {
    /// The bytes an envelope takes at most where `envelope` is unset.
    pub const ENVELOPE: usize = feed::ENVELOPE;
    /// The most bytes `envelope` allows.
    pub const ENVELOPE_ALLOWED: usize = feed::ENVELOPE_MOST;

    /// The options, where the peer is a peer's name, the inbox a source's
    /// name a URL's path carries as it is, and the envelope within its
    /// bound.
    ///
    /// # Errors
    ///
    /// The first that is not.
    pub fn check(self) -> Result<Self, Fault> {
        if !herald_core::kind_name(&self.peer) {
            return Err(Fault::at(
                "peer",
                format!(
                    "\"{}\" is not a peer's name; name a table of [peers]",
                    self.peer
                ),
            ));
        }
        if !segment(&self.inbox) {
            return Err(Fault::at(
                "inbox",
                format!(
                    "\"{}\" is not a source's name a URL carries as it is; give the name of the peer's herald source, of letters, digits, '-', '_' and '.'",
                    self.inbox
                ),
            ));
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

    /// The most bytes one envelope takes.
    #[must_use]
    pub fn envelope(&self) -> usize {
        self.envelope.map_or(Self::ENVELOPE, |envelope| envelope.0)
    }
}

/// Whether `name` is one path segment no URL escapes: letters, digits,
/// `-`, `_` and `.`, and neither `.` nor `..`.
fn segment(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

/// What a route says about a push to a peer alone: nothing yet.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PushRouteOptions {}

impl Placement for PushRouteOptions {}

impl DestinationConfig for PushOptions {
    fn kind(&self) -> &'static str {
        HERALD_PUSH
    }

    fn symbols_written(&self) -> Option<Symbols> {
        None
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn credential(&self) -> Option<&SecretRef> {
        None
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        Vec::new()
    }

    fn unread_why(&self) -> &'static str {
        UNREAD
    }

    fn pushing(&self) -> Option<Pushing<'_>> {
        Some(Pushing {
            peer: &self.peer,
            inbox: &self.inbox,
        })
    }

    fn peered(&self, peered: Peered) -> Option<Destination> {
        Some(Destination::new(Self {
            peered: Some(peered),
            ..self.clone()
        }))
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        placed.of::<PushRouteOptions>()?;
        Some((profile(), None))
    }

    fn reach(&self, placed: &Placed, _: &str, secret: Secret) -> Option<Reached> {
        placed.of::<PushRouteOptions>()?;
        let peered = self.peered.as_ref()?;
        Some(Reached::new(Push {
            peer: self.peer.clone(),
            inbox: inbox_of(&peered.url, &self.inbox),
            origin: peered.url.clone(),
            envelope: self.envelope(),
            private: peered.private,
            waits: self.waits(),
            key: secret,
        }))
    }
}

/// A push to a peer, reached: where its envelopes go, and the instance's
/// key they are proved and their requests signed by.
#[derive(Debug, Clone, PartialEq)]
pub struct Push {
    peer: String,
    inbox: String,
    origin: String,
    envelope: usize,
    private: bool,
    waits: Waits,
    key: Secret,
}

impl Reach for Push {
    fn profile(&self) -> Profile {
        profile()
    }

    fn base(&self) -> Base<'_> {
        Base::At(&self.origin)
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

    fn pushes(&self) -> Option<Pushed<'_>> {
        Some(Pushed {
            peer: &self.peer,
            inbox: &self.inbox,
            envelope: self.envelope,
            private: self.private,
            waits: self.waits,
            key: &self.key,
        })
    }
}

#[cfg(test)]
mod tests;
