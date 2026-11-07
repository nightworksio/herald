use std::future::{Future, ready};

use fastly::kv_store::{InsertMode, KVStore, KVStoreError};
use herald_ports::{Kv, KvError, Pair, Stamped, Versionstamp};

/// The metadata of an item being deleted: Fastly's delete takes no generation,
/// so a delete first claims the item at its generation under this mark.
const GONE: &str = "herald:gone";

/// A Fastly KV Store, keys joined with `/`, its generation as the versionstamp.
#[derive(Debug)]
pub(super) struct FastlyKv(String);

fn unavailable(error: &KVStoreError) -> KvError {
    KvError::Unavailable {
        reason: error.to_string(),
    }
}

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
            .map_err(|error| unavailable(&error))?
            .ok_or_else(|| KvError::Unavailable {
                reason: format!("the KV Store {} is gone", self.0),
            })
    }

    fn read(&self, key: &str) -> Result<Option<(Vec<u8>, u64)>, KvError> {
        match self.store()?.lookup(key) {
            Ok(mut found) => {
                if found.metadata().as_deref() == Some(GONE.as_bytes()) {
                    return Ok(None);
                }
                let generation = found.current_generation();
                Ok(Some((found.take_body_bytes(), generation)))
            }
            Err(KVStoreError::ItemNotFound) => Ok(None),
            Err(error) => Err(unavailable(&error)),
        }
    }

    fn written(&self, key: &str, value: Vec<u8>, expected: Option<u64>) -> Result<bool, KvError> {
        let store = self.store()?;
        let insert = store.build_insert();
        let insert = match expected {
            Some(generation) => insert.if_generation_match(generation),
            None => insert.mode(InsertMode::Add),
        };
        match insert.execute(key, value) {
            Ok(()) => Ok(true),
            Err(KVStoreError::ItemPreconditionFailed) => Ok(false),
            Err(error) => Err(unavailable(&error)),
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
            let store = self.store()?;
            let claimed = store
                .build_insert()
                .if_generation_match(generation)
                .metadata(GONE)
                .execute(&key, Vec::new());
            match claimed {
                Ok(()) => {}
                Err(KVStoreError::ItemPreconditionFailed | KVStoreError::ItemNotFound) => {
                    return Ok(false);
                }
                Err(error) => return Err(unavailable(&error)),
            }
            store.delete(&key).map_err(|error| unavailable(&error))?;
            Ok(true)
        })();
        ready(done)
    }

    fn list(&self, prefix: &[String]) -> impl Future<Output = Result<Vec<Pair>, KvError>> + Send {
        let start = format!("{}/", key_of(prefix));
        let done = (|| {
            let store = self.store()?;
            let mut pairs = Vec::new();
            for page in store.build_list().prefix(&start).iter() {
                let page = page.map_err(|error| unavailable(&error))?;
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
