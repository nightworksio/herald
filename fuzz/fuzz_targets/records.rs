//! Whatever bytes a store holds where herald kept a record, sealed or not,
//! reading every kind of record answers a record or an error.

#![no_main]

use herald_core::{DestinationName, Identity, Key, RouteId, SourceName};
use herald_ports::{
    Announcements, Cursors, Inboxes, Keyring, Ledger, Notes, Pins, Schedules, Secret, StoreError,
    Subscriptions,
};
use herald_records::{Blob, RecordBackend, RecordStore, Seal, Wrote};
use libfuzzer_sys::fuzz_target;

/// A backend whose every record holds the same hostile bytes.
#[derive(Clone)]
struct Hostile(Vec<u8>);

impl RecordBackend for Hostile {
    async fn read(&self, _: &str) -> Result<Option<Blob>, StoreError> {
        Ok(Some(Blob {
            token: "t".to_owned(),
            bytes: self.0.clone(),
        }))
    }

    async fn list(&self, directory: &str) -> Result<Vec<String>, StoreError> {
        Ok(vec![format!("{directory}/a"), format!("{directory}/b")])
    }

    async fn write(&self, _: &str, _: &[u8], _: Option<&str>) -> Result<Wrote, StoreError> {
        Ok(Wrote::Written)
    }

    async fn remove(&self, _: &str, _: &str) -> Result<Wrote, StoreError> {
        Ok(Wrote::Written)
    }
}

fn read_everything(kept: &RecordStore<Hostile>) {
    let key = Key::derive(&RouteId::new("r"), &Identity::new("i"));
    pollster::block_on(async {
        let _ = kept.entry(&key).await;
        let _ = kept.cursor(&SourceName::new("s")).await;
        let _ = kept.every_schedule().await;
        let _ = kept.subscribers(&DestinationName::new("d"), "p").await;
        let _ = kept.pinned(&RouteId::new("r")).await;
        let _ = kept.note(&key).await;
        let _ = kept.delivery(&key, "https://example.org/inbox").await;
        let _ = kept.latest("p").await;
        let _ = kept.every_latest().await;
        let _ = kept.known("https://example.org/actor#main-key").await;
    });
}

fuzz_target!(|data: &[u8]| {
    read_everything(&RecordStore::new(Hostile(data.to_vec())));
    let sealed = RecordStore::new(Hostile(data.to_vec()))
        .sealed(Seal::new(Secret::new("HERALD_STORE_KEY", "fuzz")));
    read_everything(&sealed);
});
