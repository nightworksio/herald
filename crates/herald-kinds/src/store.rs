use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use herald_core::{RecordKind, SecretRef};
use herald_ports::RecordOptions;

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

    /// Each key of its own options set above its default, with what that
    /// costs, for the doctor to name.
    fn loosened(&self) -> Vec<(&'static str, &'static str)> {
        Vec::new()
    }

    /// It, opened through `opening`, keeping its records as `options` say:
    /// sealed with their key where there is one (ADR-0005). What it opens
    /// holds nothing of its own options, and holds the opening only where it
    /// connects again later.
    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        options: RecordOptions,
    ) -> Local<'s, Result<Opened<'o>, String>>;
}

/// What a host offers a store beyond `Http`, which a kind of store may need
/// (ADR-0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Nothing beyond `Http`.
    Http,
    /// A SQLite file on a disk that outlives the run.
    Sqlite,
    /// A Postgres database.
    Postgres,
    /// A Cloudflare D1 database, through a Worker's binding.
    D1,
    /// The storage of a Cloudflare Durable Object.
    DurableObject,
    /// Deno KV.
    DenoKv,
    /// A Fastly KV Store.
    FastlyKv,
    /// The cache of the workflow run herald runs in.
    WorkflowCache,
}

/// One kind of store: the options its table reads into, how the schema
/// names it, and what it needs of its host.
pub trait StoreKind: 'static {
    /// The kind's name, as `[store] kind` writes it.
    const NAME: &str;
    /// What the kind is, as the schema describes a store of it.
    const DESCRIPTION: &str;
    /// What it needs its host to offer.
    const NEEDS: Offer = Offer::Http;
    /// Whether its table also says how long records are kept, with
    /// `keep_deliveries`, `keep_ledger` and `keep_notes`.
    const RETAINED: bool = true;

    /// The options its table reads into.
    type Options: StoreConfig + Clone;
}

/// A store's options, of any kind.
#[derive(Clone)]
pub struct ConfiguredStore(Arc<dyn StoreConfig>);

impl ConfiguredStore {
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

impl Deref for ConfiguredStore {
    type Target = dyn StoreConfig;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl fmt::Debug for ConfiguredStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for ConfiguredStore {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_any())
    }
}

impl Eq for ConfiguredStore {}

#[cfg(test)]
mod tests;
