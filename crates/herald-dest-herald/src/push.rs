use std::future::ready;

use herald_core::{
    Announcement, Chore, Delivery, Entry, Profile, SecretRef, Sent, Size, Step, Symbols,
    WaitOptions, Waits,
};
use herald_kinds::{
    Destination, DestinationConfig, DestinationKind, Fault, IdTokens, Peered, Placed, Placement,
    Placing, Proving, Pushed, Pushing, Reach, Reached, Signing, UnreadPosts,
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
    /// What proves who pushes: "key", where absent, proves each envelope
    /// and signs each request with `[federation]`'s newest key; "id-token"
    /// presents the CI run's ID token, which an app of the peer's takes,
    /// and the peer proves each envelope itself.
    pub credential: Option<PushCredential>,
    /// Where the run's ID tokens come from with `credential = "id-token"`:
    /// "actions", where absent, asks GitHub Actions for a fresh one for
    /// each push, which needs the job's `id-token: write` permission;
    /// "variables" reads them from `id_tokens`, each presented for one
    /// push in turn, as GitLab CI's `id_tokens:` sets them.
    pub id_token: Option<IdTokenSource>,
    /// The run's variables holding its ID tokens with `id_token =
    /// "variables"`, each a token for one push: `[{ secret =
    /// "HERALD_ID_TOKEN" }]` where absent. A peer whose app takes each
    /// token once refuses a second push with one; give as many as the run
    /// pushes, each its own `id_tokens:` entry with the peer's URL as its
    /// `aud`. The pushes past them wait for a later run.
    pub id_tokens: Option<Vec<SecretRef>>,
    /// What the run's ID token must be meant for with `credential =
    /// "id-token"`: the peer's URL where absent, and always on the peer's
    /// origin. A token whose `aud` names another is never sent, so no
    /// herald is handed a token meant for another, which it could present
    /// there.
    pub audience: Option<String>,
    /// The secrets the push is proved with, its credential first.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) reading: Vec<SecretRef>,
    /// The peer, as `[peers]` has it, once the config joins it.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) peered: Option<Peered>,
}

/// What proves who pushes to a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PushCredential {
    /// `[federation]`'s newest key.
    #[default]
    Key,
    /// The CI run's ID token (ADR-0064).
    IdToken,
}

/// Where a run's ID tokens come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum IdTokenSource {
    /// Asked of GitHub Actions for each push.
    #[default]
    Actions,
    /// Read from the run's variables, one for each push.
    Variables,
}

impl PushOptions {
    /// Where the run's ID token is read where `id_tokens` is unset.
    pub const ID_TOKEN: &str = "HERALD_ID_TOKEN";
    /// Where GitHub Actions takes a job's requests for ID tokens.
    pub const ACTIONS_URL: &str = "ACTIONS_ID_TOKEN_REQUEST_URL";
    /// What a job of GitHub Actions requests its ID tokens with.
    pub const ACTIONS_TOKEN: &str = "ACTIONS_ID_TOKEN_REQUEST_TOKEN";

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
        let keyed = self.credential.unwrap_or_default() == PushCredential::Key;
        let source = self.id_token.unwrap_or_default();
        let written = [
            ("audience", self.audience.is_some()),
            ("id_token", self.id_token.is_some()),
            ("id_tokens", self.id_tokens.is_some()),
        ];
        if let Some((key, _)) = written.iter().find(|(_, written)| keyed && *written) {
            return Err(Fault::at(
                *key,
                format!(
                    "{key} is read only with credential = \"id-token\"; set it, or leave {key} out"
                ),
            ));
        }
        if self.audience.as_deref() == Some("") {
            return Err(Fault::at(
                "audience",
                "the audience is empty; give the URL the peer's tokens name, or leave audience out",
            ));
        }
        let reading = match (source, self.id_tokens.clone()) {
            _ if keyed => Vec::new(),
            (IdTokenSource::Actions, Some(_)) => {
                return Err(Fault::at(
                    "id_tokens",
                    "id_tokens are read only with id_token = \"variables\"; set it, or leave id_tokens out",
                ));
            }
            (IdTokenSource::Actions, None) => [Self::ACTIONS_TOKEN, Self::ACTIONS_URL]
                .map(|secret| SecretRef {
                    secret: secret.to_owned(),
                })
                .to_vec(),
            (IdTokenSource::Variables, Some(tokens)) if tokens.is_empty() => {
                return Err(Fault::at(
                    "id_tokens",
                    "no ID token is read, so nothing is pushed; give one variable for each push, or leave id_tokens out",
                ));
            }
            (IdTokenSource::Variables, tokens) => tokens.unwrap_or_else(|| {
                vec![SecretRef {
                    secret: Self::ID_TOKEN.to_owned(),
                }]
            }),
        };
        Ok(Self { reading, ..self })
    }

    /// The most bytes one envelope takes.
    #[must_use]
    pub fn envelope(&self) -> usize {
        self.envelope.map_or(Self::ENVELOPE, |envelope| envelope.0)
    }
}

/// The origin of `url`, where it is a URL with a host; so where a CI's
/// address for ID tokens is reached is its origin alone, and a malformed
/// one is reached nowhere, never quoted.
fn origin(url: &str) -> Option<String> {
    herald_core::Address::parse(url).map(|address| address.origin())
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
        self.reading.first()
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        self.reading.iter().collect()
    }

    fn reads(&self) -> Vec<&SecretRef> {
        self.reading.iter().skip(1).collect()
    }

    fn unread_why(&self) -> &'static str {
        UNREAD
    }

    fn pushing(&self) -> Option<Pushing<'_>> {
        Some(Pushing {
            peer: &self.peer,
            inbox: &self.inbox,
            id_token: !self.reading.is_empty(),
            audience: self.audience.as_deref(),
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

    fn reach(&self, placed: &Placed, base: &str, secret: Secret) -> Option<Reached> {
        self.reach_reading(placed, base, (secret, Vec::new()))
    }

    fn reach_reading(
        &self,
        placed: &Placed,
        _: &str,
        (secret, read): (Secret, Vec<Secret>),
    ) -> Option<Reached> {
        placed.of::<PushRouteOptions>()?;
        let peered = self.peered.as_ref()?;
        let proof = match (self.reading.is_empty(), self.id_token.unwrap_or_default()) {
            (true, _) => Proof::Key(secret),
            (false, IdTokenSource::Actions) => Proof::Requested {
                url: read.into_iter().next()?,
                token: secret,
            },
            (false, IdTokenSource::Variables) => Proof::Read(
                std::iter::once(secret).chain(read).collect(),
                peered.presented.clone(),
            ),
        };
        let origins = std::iter::once(peered.url.clone())
            .chain(match &proof {
                Proof::Requested { url, .. } => origin(url.expose()),
                Proof::Key(_) | Proof::Read(..) => None,
            })
            .collect();
        Some(Reached::new(Push {
            peer: self.peer.clone(),
            inbox: inbox_of(&peered.url, &self.inbox),
            origins,
            envelope: self.envelope(),
            private: peered.private,
            waits: self.waits(),
            proof,
            audience: self.audience.clone().unwrap_or_else(|| peered.url.clone()),
        }))
    }
}

/// A push to a peer, reached: where its envelopes go, and the instance's
/// key they are proved and their requests signed by, or where the run's
/// ID tokens presented with them come from.
#[derive(Debug, Clone, PartialEq)]
pub struct Push {
    peer: String,
    inbox: String,
    origins: Vec<String>,
    envelope: usize,
    private: bool,
    waits: Waits,
    proof: Proof,
    audience: String,
}

/// What proves who pushes, reached.
#[derive(Debug, Clone, PartialEq)]
enum Proof {
    /// The instance's newest key.
    Key(Secret),
    /// A token asked of GitHub Actions for each push.
    Requested {
        /// Where.
        url: Secret,
        /// With what.
        token: Secret,
    },
    /// The run's tokens read from its variables, and which of them were
    /// presented.
    Read(Vec<Secret>, herald_kinds::Presented),
}

impl Reach for Push {
    fn profile(&self) -> Profile {
        profile()
    }

    fn base(&self) -> Base<'_> {
        match self.origins.as_slice() {
            [origin] => Base::At(origin),
            origins => Base::Among(origins),
        }
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
            proving: match &self.proof {
                Proof::Key(key) => Proving::Key(key),
                Proof::Requested { url, token } => Proving::IdToken {
                    tokens: IdTokens::Requested { url, token },
                    audience: &self.audience,
                },
                Proof::Read(tokens, presented) => Proving::IdToken {
                    tokens: IdTokens::Read { tokens, presented },
                    audience: &self.audience,
                },
            },
        })
    }
}

#[cfg(test)]
mod tests;
