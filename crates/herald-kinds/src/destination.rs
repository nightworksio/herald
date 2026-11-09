use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use herald_core::{Answering, Profile, SecretRef, Symbols, WaitOptions, Waits};
use herald_ports::{Boxed, DynSecrets, Secret, SecretError};

use crate::{Interacted, Itself, Placed, Reached};

/// Why a route's posts are not read back, where nothing else says why.
pub const UNREAD: &str = "a webhook cannot read back its posts";

/// One destination's table in `herald.toml`, read: what every kind of
/// destination's options are to the config and the flows that read it.
pub trait DestinationConfig: fmt::Debug + Send + Sync + Itself {
    /// The kind's name, as `[destinations.<name>] kind` writes it.
    fn kind(&self) -> &'static str;

    /// The characters its posts write where its markup has none, where the
    /// config sets them.
    fn symbols_written(&self) -> Option<Symbols>;

    /// The characters its posts write where its markup has none.
    fn symbols(&self) -> Symbols {
        self.symbols_written().unwrap_or_default()
    }

    /// The waits its config sets, each still to be checked.
    fn waits_written(&self) -> Option<WaitOptions>;

    /// How long its platform has it wait where it names no wait.
    fn platform_waits(&self) -> Waits {
        Waits::DEFAULT
    }

    /// How long it waits before sending again where its platform names no
    /// wait: the config's, else the platform's own.
    fn waits(&self) -> Waits {
        Waits::of(self.waits_written(), self.platform_waits())
    }

    /// The secret it reaches its platform with.
    fn credential(&self) -> Option<&SecretRef>;

    /// Every secret it names.
    fn secret_refs(&self) -> Vec<&SecretRef>;

    /// How it answers commands, where it takes any.
    fn answering(&self) -> Option<Answering> {
        None
    }

    /// How the commands it takes reach herald, where it takes any
    /// (ADR-0016).
    fn carried(&self) -> Option<Carried> {
        None
    }

    /// Why a route's posts through it are not read back, where its profile
    /// says they are not.
    fn unread_why(&self) -> &'static str {
        UNREAD
    }

    /// Whether it is one of herald's own actors: its followers are kept, its
    /// notes served, and its credential is the key it signs with.
    fn acts(&self) -> bool {
        false
    }

    /// The domains its sessions connect under, where it holds any open
    /// (ADR-0016).
    fn session_domains(&self) -> Vec<&str> {
        vec![]
    }

    /// What a route placed as `placed` can do through it, as it is reached
    /// and, where a bot could do more, as a bot: where `placed` are options
    /// of its kind.
    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)>;

    /// It, reached for a route placed as `placed` with its credential
    /// `secret` and herald's own `base` URL: where `placed` are options of
    /// its kind.
    fn reach(&self, placed: &Placed, base: &str, secret: Secret) -> Option<Reached>;

    /// How it takes commands, reached with `credential` and its other
    /// secrets read through `secrets`, where it takes any.
    fn interacts<'a>(
        &'a self,
        _credential: &'a Secret,
        _secrets: &'a dyn DynSecrets,
    ) -> Boxed<'a, Result<Option<Interacted>, SecretError>> {
        Box::pin(std::future::ready(Ok(None)))
    }
}

/// How a destination's commands reach herald (ADR-0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Carried {
    /// Whether they are posted to herald's endpoint, or else carried over a
    /// session herald holds open.
    pub posted: bool,
    /// What carries them, as a sentence names it.
    pub what: &'static str,
    /// The session that would carry them instead, and how to choose it.
    pub session: &'static str,
}

/// A destination's options, of any kind.
#[derive(Clone)]
pub struct Destination(Arc<dyn DestinationConfig>);

impl Destination {
    /// `options`, as a destination.
    #[must_use]
    pub fn new(options: impl DestinationConfig) -> Self {
        Self(Arc::new(options))
    }

    /// The options as their own kind's, where they are a `T`.
    #[must_use]
    pub fn of<T: DestinationConfig>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }
}

impl Deref for Destination {
    type Target = dyn DestinationConfig;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl fmt::Debug for Destination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for Destination {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_any())
    }
}

impl Eq for Destination {}

#[cfg(test)]
mod tests;
