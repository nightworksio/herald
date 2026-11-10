use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use herald_core::{Cursor, Event, Kind, Polled, Scheme, SecretRef};
use herald_ports::{Boxed, DynHttp, Secret};

use crate::{Fault, Itself};

/// One source's table in `herald.toml`, read: what every kind of source's
/// options are to the config and the flows that read it.
pub trait SourceConfig: fmt::Debug + Send + Sync + Itself {
    /// The kind's name, as `[sources.<name>] kind` writes it.
    fn kind(&self) -> &'static str;

    /// The token it reads its platform with, where it names one.
    fn token(&self) -> Option<&SecretRef>;

    /// The secret its webhooks are signed with, where it takes them.
    fn webhook(&self) -> Option<&SecretRef> {
        None
    }

    /// Every secret it names.
    fn secret_refs(&self) -> Vec<&SecretRef> {
        self.token().into_iter().chain(self.webhook()).collect()
    }

    /// The kinds of event it produces.
    fn kinds(&self) -> &'static [Kind];

    /// How many of its newest events the first poll announces (ADR-0003).
    fn backfill(&self) -> usize;

    /// Who signs its webhooks, and how, where it takes them (ADR-0007).
    fn scheme(&self) -> Option<Scheme> {
        None
    }

    /// What the webhook delivery `payload` with `headers` asks herald to
    /// announce, where it takes webhooks.
    ///
    /// # Errors
    ///
    /// Why the delivery could not be read.
    fn hooked(&self, _headers: &[(&str, &str)], _payload: &[u8]) -> Result<Hooked, String> {
        Ok(Hooked::Nothing(UNHOOKED))
    }

    /// What the doctor says of every run that serves it, where its options
    /// leave something to its platform.
    fn note(&self) -> Option<&'static str> {
        None
    }

    /// Whether a name it announces can move to another version, as a
    /// registry's `latest` tag can.
    fn moving_tags(&self) -> bool {
        false
    }

    /// What polls it, reading with `token` where it has one.
    fn poller(&self, token: Option<Secret>) -> Arc<dyn Poll>;
}

/// Why a source that takes no webhooks announces nothing from one.
pub const UNHOOKED: &str = "this source takes no webhooks";

/// What a webhook delivery asks herald to announce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hooked {
    /// The event it publishes.
    Event(Event),
    /// Nothing, and why.
    Nothing(&'static str),
}

/// One kind of source: the table it reads, the options it reads into, and
/// how the schema names it.
pub trait SourceKind: 'static {
    /// The kind's name, as `[sources.<name>] kind` writes it.
    const NAME: &str;
    /// What the kind is, as the schema describes a source of it.
    const DESCRIPTION: &str;

    /// The table it reads.
    type File;
    /// The options its table reads into.
    type Options: SourceConfig + Clone;

    /// The options `file` gives, where they are sound.
    ///
    /// # Errors
    ///
    /// The fault that keeps the source out of the config.
    fn checked(file: Self::File) -> Result<Self::Options, Fault>;
}

/// One source, ready to poll.
pub trait Poll: fmt::Debug + Send + Sync {
    /// The base URL of everything it asks, the only origin its `Http` may
    /// reach (ADR-0007).
    fn api(&self) -> &str;

    /// How many identities its cursor remembers, newest first.
    fn remembered(&self) -> usize;

    /// What it answers through `http` since `cursor`, or nothing where it
    /// says nothing changed.
    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        cursor: Option<&'a Cursor>,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>>;
}

/// Why a poll found nothing to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unpolled {
    /// The source could not be reached.
    Unreachable(String),
    /// Its answer could not be read.
    Unreadable(String),
}

impl Unpolled {
    /// The source could not be reached, as `error` says.
    pub fn unreachable(error: impl fmt::Display) -> Self {
        Self::Unreachable(error.to_string())
    }

    /// Its answer could not be read, as `error` says.
    pub fn unreadable(error: impl fmt::Display) -> Self {
        Self::Unreadable(error.to_string())
    }
}

/// A source's options, of any kind.
#[derive(Clone)]
pub struct Source(Arc<dyn SourceConfig>);

impl Source {
    /// `options`, as a source.
    #[must_use]
    pub fn new(options: impl SourceConfig) -> Self {
        Self(Arc::new(options))
    }

    /// The options as their own kind's, where they are a `T`.
    #[must_use]
    pub fn of<T: SourceConfig>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }
}

impl Deref for Source {
    type Target = dyn SourceConfig;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_any())
    }
}

impl Eq for Source {}

#[cfg(test)]
mod tests;
