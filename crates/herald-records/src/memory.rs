use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::sync::{Mutex, PoisonError};

use herald_ports::StoreError;

use crate::records::{Blob, Records, Wrote};

/// What memory holds: how many writes it has taken, and each record with
/// the write that made it.
#[derive(Debug, Default)]
struct Held {
    made: u64,
    records: BTreeMap<String, (u64, Vec<u8>)>,
}

/// Records kept in memory for one run, as a dry run keeps them.
#[derive(Debug, Default)]
pub struct Memory {
    held: Mutex<Held>,
}

impl Memory {
    fn with<T>(&self, change: impl FnOnce(&mut Held) -> T) -> T {
        change(&mut self.held.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Records for Memory {
    fn read(&self, path: &str) -> impl Future<Output = Result<Option<Blob>, StoreError>> + Send {
        ready(Ok(self.with(|held| {
            held.records.get(path).map(|(token, bytes)| Blob {
                token: token.to_string(),
                bytes: bytes.clone(),
            })
        })))
    }

    fn list(
        &self,
        directory: &str,
    ) -> impl Future<Output = Result<Vec<String>, StoreError>> + Send {
        let inside = format!("{directory}/");
        ready(Ok(self.with(|held| {
            held.records
                .keys()
                .filter(|path| path.starts_with(&inside))
                .cloned()
                .collect()
        })))
    }

    fn write(
        &self,
        path: &str,
        bytes: &[u8],
        replacing: Option<&str>,
    ) -> impl Future<Output = Result<Wrote, StoreError>> + Send {
        ready(Ok(self.with(|held| {
            let current = held.records.get(path).map(|(token, _)| token.to_string());
            if current.as_deref() != replacing {
                return Wrote::Conflict;
            }
            held.made += 1;
            held.records
                .insert(path.to_owned(), (held.made, bytes.to_vec()));
            Wrote::Written
        })))
    }

    fn remove(
        &self,
        path: &str,
        replacing: &str,
    ) -> impl Future<Output = Result<Wrote, StoreError>> + Send {
        ready(Ok(self.with(|held| match held.records.get(path) {
            Some((token, _)) if token.to_string() != replacing => Wrote::Conflict,
            _ => {
                held.records.remove(path);
                Wrote::Written
            }
        })))
    }
}
