use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::sync::{Mutex, PoisonError};

use herald_ports::{Keyring, KnownKey, StoreError};
use jiff::Timestamp;

/// The keys held, each with when it was last used.
#[derive(Debug, Default)]
struct Held {
    used: u64,
    keys: BTreeMap<String, (u64, KnownKey)>,
}

/// Keys kept in memory, at most a number of them: the key used longest ago
/// is forgotten to make room for another.
#[derive(Debug)]
pub struct MemoryKeyring {
    most: usize,
    held: Mutex<Held>,
}

impl MemoryKeyring {
    /// A keyring holding at most `most` keys.
    #[must_use]
    pub fn new(most: usize) -> Self {
        Self {
            most,
            held: Mutex::new(Held::default()),
        }
    }

    fn with<T>(&self, change: impl FnOnce(&mut Held) -> T) -> T {
        change(&mut self.held.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Keyring for MemoryKeyring {
    fn known(&self, id: &str) -> impl Future<Output = Result<Option<KnownKey>, StoreError>> + Send {
        ready(Ok(self.with(|held| {
            held.used += 1;
            let used = held.used;
            held.keys.get_mut(id).map(|(last, key)| {
                *last = used;
                key.clone()
            })
        })))
    }

    fn know(
        &self,
        id: &str,
        key: &KnownKey,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        let most = self.most;
        self.with(|held| {
            held.used += 1;
            held.keys.insert(id.to_owned(), (held.used, key.clone()));
            while held.keys.len() > most {
                let oldest = held
                    .keys
                    .iter()
                    .min_by_key(|(_, (used, _))| *used)
                    .map(|(id, _)| id.clone());
                if let Some(oldest) = oldest {
                    held.keys.remove(&oldest);
                }
            }
        });
        ready(Ok(()))
    }

    fn forget_before(
        &self,
        before: Timestamp,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.with(|held| held.keys.retain(|_, (_, key)| key.fetched >= before));
        ready(Ok(()))
    }
}
