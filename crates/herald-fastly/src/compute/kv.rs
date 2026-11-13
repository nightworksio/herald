use std::future::{Future, ready};
use std::time::Duration;

use fastly::kv_store::{InsertMode, KVStore, KVStoreError};
use herald_ports::{Kv, KvError, Pair, Stamped, Versionstamp};

/// The metadata of a deleted item. Fastly's delete takes no generation, so a
/// delete replaces the item at its generation with an empty item under this
/// mark, which reads as absent and which a write replaces at its generation.
const GONE: &str = "herald:gone";

/// How long a deleted item's mark is kept before Fastly expires it.
const GONE_FOR: Duration = Duration::from_secs(3_600);

/// An item as the store holds it: a record, or a record being deleted.
struct Held {
    value: Vec<u8>,
    generation: u64,
    gone: bool,
}

/// A Fastly KV Store, keys joined with `/`, its generation as the versionstamp.
#[derive(Debug)]
pub(super) struct FastlyKv(String);

fn key_of(key: &[String]) -> String {
    key.join("/")
}

impl FastlyKv {
    pub(super) fn open(name: &str) -> Result<Self, String> {
        match KVStore::open(name) {
            Ok(Some(_)) => Ok(Self(name.to_owned())),
            Ok(None) => Err(format!(
                "the service is not linked to a KV Store named {name}"
            )),
            Err(error) => Err(error.to_string()),
        }
    }

    fn store(&self) -> Result<KVStore, KvError> {
        KVStore::open(&self.0)
            .map_err(KvError::unavailable)?
            .ok_or_else(|| KvError::unavailable(format_args!("the KV Store {} is gone", self.0)))
    }

    fn held(&self, key: &str) -> Result<Option<Held>, KvError> {
        match self.store()?.lookup(key) {
            Ok(mut found) => Ok(Some(Held {
                gone: found.metadata().as_deref() == Some(GONE.as_bytes()),
                generation: found.current_generation(),
                value: found.take_body_bytes(),
            })),
            Err(KVStoreError::ItemNotFound) => Ok(None),
            Err(error) => Err(KvError::unavailable(error)),
        }
    }

    fn read(&self, key: &str) -> Result<Option<(Vec<u8>, u64)>, KvError> {
        Ok(self
            .held(key)?
            .filter(|held| !held.gone)
            .map(|held| (held.value, held.generation)))
    }

    /// Writes `value` at `key` if the key holds the record at `expected`, or
    /// holds none: a deleted record's mark is replaced at its generation.
    fn written(&self, key: &str, value: Vec<u8>, expected: Option<u64>) -> Result<bool, KvError> {
        let expected = match expected {
            Some(generation) => Some(generation),
            None => match self.held(key)? {
                Some(held) if held.gone => Some(held.generation),
                Some(_) => return Ok(false),
                None => None,
            },
        };
        let store = self.store()?;
        let insert = store.build_insert();
        let insert = match expected {
            Some(generation) => insert.if_generation_match(generation),
            None => insert.mode(InsertMode::Add),
        };
        match insert.execute(key, value) {
            Ok(()) => Ok(true),
            Err(KVStoreError::ItemPreconditionFailed | KVStoreError::ItemNotFound) => Ok(false),
            Err(error) => Err(KvError::unavailable(error)),
        }
    }
}

fn generation_of(stamp: &Versionstamp) -> Option<u64> {
    stamp.0.parse().ok()
}

impl Kv for FastlyKv {
    fn get(&self, key: &[String]) -> impl Future<Output = Result<Option<Stamped>, KvError>> + Send {
        ready(self.read(&key_of(key)).map(|found| {
            found.map(|(value, generation)| Stamped {
                value,
                versionstamp: Versionstamp(generation.to_string()),
            })
        }))
    }

    fn set_if(
        &self,
        key: &[String],
        value: Vec<u8>,
        expected: Option<Versionstamp>,
    ) -> impl Future<Output = Result<Option<Versionstamp>, KvError>> + Send {
        let key = key_of(key);
        let done = (|| {
            let generation = match &expected {
                Some(stamp) => match generation_of(stamp) {
                    Some(generation) => Some(generation),
                    None => return Ok(None),
                },
                None => None,
            };
            if !self.written(&key, value, generation)? {
                return Ok(None);
            }
            Ok(self
                .read(&key)?
                .map(|(_, generation)| Versionstamp(generation.to_string())))
        })();
        ready(done)
    }

    fn delete_if(
        &self,
        key: &[String],
        expected: Versionstamp,
    ) -> impl Future<Output = Result<bool, KvError>> + Send {
        let key = key_of(key);
        let done = (|| {
            let Some(generation) = generation_of(&expected) else {
                return Ok(false);
            };
            let marked = self
                .store()?
                .build_insert()
                .if_generation_match(generation)
                .metadata(GONE)
                .time_to_live(GONE_FOR)
                .execute(&key, Vec::new());
            match marked {
                Ok(()) => Ok(true),
                Err(KVStoreError::ItemPreconditionFailed | KVStoreError::ItemNotFound) => Ok(false),
                Err(error) => Err(KvError::unavailable(error)),
            }
        })();
        ready(done)
    }

    fn list(&self, prefix: &[String]) -> impl Future<Output = Result<Vec<Pair>, KvError>> + Send {
        let start = format!("{}/", key_of(prefix));
        let done = (|| {
            let store = self.store()?;
            let mut pairs = Vec::new();
            for page in store.build_list().prefix(&start).iter() {
                let page = page.map_err(KvError::unavailable)?;
                for name in page.keys() {
                    if let Some((value, _)) = self.read(name)? {
                        pairs.push(Pair {
                            key: name.split('/').map(ToOwned::to_owned).collect(),
                            value,
                        });
                    }
                }
            }
            Ok(pairs)
        })();
        ready(done)
    }
}
