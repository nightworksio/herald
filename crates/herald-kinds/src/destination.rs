use std::any::Any;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use herald_core::{Answering, SecretRef, Symbols, WaitOptions, Waits};

/// One destination's table in `herald.toml`, read: what every kind of
/// destination's options are to the config and the flows that read it.
pub trait DestinationConfig: fmt::Debug + Send + Sync + Any {
    /// The kind's name, as `[destinations.<name>] kind` writes it.
    fn kind(&self) -> &'static str;

    /// The characters its posts write where its markup has none, where the
    /// config sets them.
    fn symbols(&self) -> Option<Symbols>;

    /// The waits its config sets, each still to be checked.
    fn waits_written(&self) -> Option<WaitOptions>;

    /// How long its platform has it wait where it names no wait.
    fn platform_waits(&self) -> Waits {
        Waits::DEFAULT
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

    /// Whether it is one of herald's own actors: its followers are kept, its
    /// notes served, and its credential is the key it signs with.
    fn acts(&self) -> bool {
        false
    }

    /// The domains its sessions connect under, where it holds any open
    /// (ADR-0016).
    fn session_domains(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Itself, for code that reads one kind's own options.
    fn as_any(&self) -> &dyn Any;

    /// Whether `other` is the same options of the same kind.
    fn same(&self, other: &dyn Any) -> bool;
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

    /// The characters its posts write where its markup has none.
    #[must_use]
    pub fn symbols(&self) -> Symbols {
        self.0.symbols().unwrap_or_default()
    }

    /// How long it waits before sending again where its platform names no
    /// wait: the config's, else the platform's own.
    #[must_use]
    pub fn waits(&self) -> Waits {
        Waits::of(self.0.waits_written(), self.0.platform_waits())
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
