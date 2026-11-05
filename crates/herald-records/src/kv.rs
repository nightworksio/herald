use herald_ports::{Kv, KvError, StoreError, Versionstamp};

use crate::records::{Blob, Records, Wrote};

/// Records kept one value per path in a key-value store (ADR-0005): each
/// path a key of its segments under the store's prefix, each value's
/// versionstamp the token a write names to replace it.
#[derive(Debug)]
pub struct KvRecords<K> {
    kv: K,
    prefix: Vec<String>,
}

fn unavailable(error: &KvError) -> StoreError {
    StoreError::Unavailable {
        reason: error.to_string(),
    }
}

impl<K: Kv> KvRecords<K> {
    /// The records `kv` keeps under the key `prefix`.
    #[must_use]
    pub fn new(kv: K, prefix: &str) -> Self {
        Self {
            kv,
            prefix: prefix
                .split('/')
                .filter(|part| !part.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
        }
    }

    fn key(&self, path: &str) -> Vec<String> {
        self.prefix
            .iter()
            .cloned()
            .chain(path.split('/').map(ToOwned::to_owned))
            .collect()
    }
}

impl<K: Kv> Records for KvRecords<K> {
    async fn read(&self, path: &str) -> Result<Option<Blob>, StoreError> {
        let stamped = self
            .kv
            .get(&self.key(path))
            .await
            .map_err(|error| unavailable(&error))?;
        Ok(stamped.map(|stamped| Blob {
            token: stamped.versionstamp.0,
            bytes: stamped.value,
        }))
    }

    async fn list(&self, directory: &str) -> Result<Vec<String>, StoreError> {
        let pairs = self
            .kv
            .list(&self.key(directory))
            .await
            .map_err(|error| unavailable(&error))?;
        let mut paths: Vec<String> = pairs
            .into_iter()
            .filter_map(|pair| {
                let path = pair.key.get(self.prefix.len()..)?.join("/");
                (path.len() > directory.len()).then_some(path)
            })
            .collect();
        paths.sort();
        Ok(paths)
    }

    async fn write(
        &self,
        path: &str,
        bytes: &[u8],
        replacing: Option<&str>,
    ) -> Result<Wrote, StoreError> {
        let expected = replacing.map(|token| Versionstamp(token.to_owned()));
        let written = self
            .kv
            .set_if(&self.key(path), bytes.to_vec(), expected)
            .await
            .map_err(|error| unavailable(&error))?;
        Ok(if written.is_some() {
            Wrote::Written
        } else {
            Wrote::Conflict
        })
    }

    async fn remove(&self, path: &str, replacing: &str) -> Result<Wrote, StoreError> {
        let key = self.key(path);
        let deleted = self
            .kv
            .delete_if(&key, Versionstamp(replacing.to_owned()))
            .await
            .map_err(|error| unavailable(&error))?;
        if deleted {
            return Ok(Wrote::Written);
        }
        let left = self
            .kv
            .get(&key)
            .await
            .map_err(|error| unavailable(&error))?;
        Ok(if left.is_none() {
            Wrote::Written
        } else {
            Wrote::Conflict
        })
    }
}

