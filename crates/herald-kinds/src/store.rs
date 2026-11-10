use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use herald_core::{RecordKind, SecretRef};
use herald_ports::Secret;

use crate::{Itself, Local, Opened, Opening};

/// The `[store]` table in `herald.toml`, read: what every kind of store's
/// options are to the config and the flows that read it.
pub trait StoreConfig: fmt::Debug + Send + Sync + Itself {
    /// The kind's name, as `[store] kind` writes it.
    fn kind(&self) -> &'static str;

    /// The secret it seals its records with, where it names one (ADR-0005).
    fn key(&self) -> Option<&SecretRef>;

    /// Every secret it names besides its key.
    fn named(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Whether it keeps records of `kind`.
    fn keeps(&self, _kind: RecordKind) -> bool {
        true
    }

    /// How it would keep records of `kind`, where its own options can make
    /// it.
    fn kept_by(&self, _kind: RecordKind) -> Option<String> {
        None
    }

    /// Whether it keeps nothing itself and reads the routes' own posts.
    fn reads_posts(&self) -> bool {
        false
    }

    /// Whether it keeps each webhook delivery it admits, so a replay is
    /// refused before it reaches a parser (ADR-0013).
    fn keeps_deliveries(&self) -> bool {
        true
    }

    /// What the doctor says of every run that keeps its records in it.
    fn note(&self) -> Option<&'static str> {
        None
    }

    /// The key under `[store]` whose value no run can keep records with,
    /// and why, where its options name one.
    fn withheld(&self) -> Option<(&'static str, &'static str)> {
        None
    }

    /// It, opened through `opening`, every record it keeps sealed with the
    /// key `seal` where there is one (ADR-0005). What it opens holds nothing of its options,
    /// and holds the opening only where it connects again later.
    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        seal: Option<Secret>,
    ) -> Local<'s, Result<Opened<'o>, String>>;
}

/// One kind of store: the options its table reads into, and how the schema
/// names it.
pub trait StoreKind: 'static {
    /// The kind's name, as `[store] kind` writes it.
    const NAME: &str;
    /// What the kind is, as the schema describes a store of it.
    const DESCRIPTION: &str;
    /// Whether its table also says how long records are kept, with
    /// `keep_deliveries`, `keep_ledger` and `keep_notes`.
    const RETAINED: bool = true;

    /// The options its table reads into.
    type Options: StoreConfig + Clone;
}

/// A store's options, of any kind.
#[derive(Clone)]
pub struct Store(Arc<dyn StoreConfig>);

impl Store {
    /// `options`, as a store.
    #[must_use]
    pub fn new(options: impl StoreConfig) -> Self {
        Self(Arc::new(options))
    }

    /// The options as their own kind's, where they are a `T`.
    #[must_use]
    pub fn of<T: StoreConfig>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }

    /// The name of every secret it names.
    #[must_use]
    pub fn secrets(&self) -> Vec<&str> {
        let mut named = self.0.named();
        named.extend(self.0.key().map(|key| key.secret.as_str()));
        named
    }
}

impl Deref for Store {
    type Target = dyn StoreConfig;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_any())
    }
}

impl Eq for Store {}

#[cfg(test)]
mod tests;
