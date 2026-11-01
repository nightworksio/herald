use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_ports::{Http, Secret, StoreError};
use herald_records::{Blob, Kept, Records, Wrote};
use serde::{Deserialize, Serialize};

/// The prefix of every generation's key where none is given.
const PREFIX: &str = "herald-state";

/// What connects to the origin a signed blob URL names.
pub type Blobs<'a, H> = Box<dyn Fn(&str) -> Result<H, String> + Send + Sync + 'a>;

/// Where the records are kept: the Actions cache of the run, reached at its
/// results service with its runtime token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsCache {
    pub(crate) results: String,
    pub(crate) token: Secret,
    pub(crate) prefix: String,
}

impl ActionsCache {
    /// The cache of the results service at `results`, reached with the run's
    /// runtime `token`, its generations keyed by `prefix` or `herald-state`.
    #[must_use]
    pub fn new(results: &str, token: Secret, prefix: Option<&str>) -> Self {
        Self {
            results: results.trim_end_matches('/').to_owned(),
            token,
            prefix: prefix.unwrap_or(PREFIX).to_owned(),
        }
    }

    /// The base URL of the results service.
    #[must_use]
    pub fn results(&self) -> &str {
        &self.results
    }

    /// The store kept in the cache, reaching the results service through
    /// `service` and each signed blob URL's origin through what `blobs`
    /// connects.
    #[must_use]
    pub fn store<H: Http + Send>(
        self,
        service: H,
        blobs: Blobs<'_, H>,
    ) -> ActionsCacheStore<'_, H> {
        Kept::new(CacheRecords {
            cache: self,
            service,
            blobs,
            current: Mutex::new(None),
        })
    }
}

/// One record in a generation: the generation that wrote it, and its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Held {
    pub(crate) written: u64,
    pub(crate) bytes: String,
}

/// One generation of the document holding every record.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct Generation {
    pub(crate) number: u64,
    pub(crate) records: BTreeMap<String, Held>,
}

/// An [`ActionsCache`] reached through `Http`: where an
/// [`ActionsCacheStore`] keeps its records.
pub struct CacheRecords<'a, H> {
    pub(crate) cache: ActionsCache,
    pub(crate) service: H,
    pub(crate) blobs: Blobs<'a, H>,
    current: Mutex<Option<Generation>>,
}

impl<H> std::fmt::Debug for CacheRecords<'_, H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheRecords")
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

/// The store an [`ActionsCache`] keeps.
pub type ActionsCacheStore<'a, H> = Kept<CacheRecords<'a, H>>;

fn corrupt(path: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        reason: format!("{path}: {error}"),
    }
}

impl<H: Http + Send> CacheRecords<'_, H> {
    fn remembered(&self) -> Option<Generation> {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn remember(&self, generation: Option<Generation>) {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = generation;
    }

    async fn generation(&self) -> Result<Generation, StoreError> {
        if let Some(generation) = self.remembered() {
            return Ok(generation);
        }
        let generation = self.latest().await?.unwrap_or_default();
        self.remember(Some(generation.clone()));
        Ok(generation)
    }

    async fn written(&self, next: Generation) -> Result<Wrote, StoreError> {
        let wrote = self.publish(&next).await?;
        self.remember((wrote == Wrote::Written).then_some(next));
        Ok(wrote)
    }

    fn conflict(&self) -> Wrote {
        self.remember(None);
        Wrote::Conflict
    }
}

impl<H: Http + Send> Records for CacheRecords<'_, H> {
    async fn read(&self, path: &str) -> Result<Option<Blob>, StoreError> {
        let generation = self.generation().await?;
        let Some(held) = generation.records.get(path) else {
            return Ok(None);
        };
        let bytes = STANDARD
            .decode(&held.bytes)
            .map_err(|error| corrupt(path, error))?;
        Ok(Some(Blob {
            token: held.written.to_string(),
            bytes,
        }))
    }

    async fn list(&self, directory: &str) -> Result<Vec<String>, StoreError> {
        let inside = format!("{directory}/");
        Ok(self
            .generation()
            .await?
            .records
            .into_keys()
            .filter(|path| path.starts_with(&inside))
            .collect())
    }

    async fn write(
        &self,
        path: &str,
        bytes: &[u8],
        replacing: Option<&str>,
    ) -> Result<Wrote, StoreError> {
        let mut next = self.generation().await?;
        let current = next.records.get(path).map(|held| held.written.to_string());
        if current.as_deref() != replacing {
            return Ok(self.conflict());
        }
        next.number += 1;
        next.records.insert(
            path.to_owned(),
            Held {
                written: next.number,
                bytes: STANDARD.encode(bytes),
            },
        );
        self.written(next).await
    }

    async fn remove(&self, path: &str, replacing: &str) -> Result<Wrote, StoreError> {
        let mut next = self.generation().await?;
        let Some(held) = next.records.remove(path) else {
            return Ok(Wrote::Written);
        };
        if held.written.to_string() != replacing {
            return Ok(self.conflict());
        }
        next.number += 1;
        self.written(next).await
    }
}
