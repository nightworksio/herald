//! The store kept over an in-memory backend, through the conformance suite
//! every store passes, and through the races and corruption a backend meets.

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the fake answers at once, and is written as the async trait it fakes"
)]
mod kept {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use herald_core::{DestinationName, Entry, Identity, Key, RouteId, Subscription};
    use herald_ports::{Store, StoreError, Version, Written};
    use herald_records::{Blob, Kept, Records, Wrote};
    use herald_testing::store_conformance;

    #[derive(Debug, Default)]
    struct Memory {
        files: Mutex<BTreeMap<String, (u64, Vec<u8>)>>,
        made: Mutex<u64>,
        races: Mutex<u32>,
        listed: Vec<String>,
    }

    impl Memory {
        fn racing(races: u32) -> Self {
            Self {
                races: Mutex::new(races),
                ..Self::default()
            }
        }

        fn raced(&self) -> bool {
            let mut races = self.races.lock().unwrap();
            if *races == 0 {
                return false;
            }
            *races -= 1;
            true
        }

        fn holding(path: &str, bytes: &[u8]) -> Self {
            let memory = Self::default();
            memory
                .files
                .lock()
                .unwrap()
                .insert(path.to_owned(), (1, bytes.to_vec()));
            memory
        }
    }

    impl Records for Memory {
        async fn read(&self, path: &str) -> Result<Option<Blob>, StoreError> {
            Ok(self
                .files
                .lock()
                .unwrap()
                .get(path)
                .map(|(token, bytes)| Blob {
                    token: token.to_string(),
                    bytes: bytes.clone(),
                }))
        }

        async fn list(&self, directory: &str) -> Result<Vec<String>, StoreError> {
            let mut found: Vec<String> = self
                .files
                .lock()
                .unwrap()
                .keys()
                .filter(|path| path.starts_with(&format!("{directory}/")))
                .cloned()
                .collect();
            found.extend(self.listed.iter().cloned());
            Ok(found)
        }

        async fn write(
            &self,
            path: &str,
            bytes: &[u8],
            replacing: Option<&str>,
        ) -> Result<Wrote, StoreError> {
            if self.raced() {
                return Ok(Wrote::Conflict);
            }
            let mut files = self.files.lock().unwrap();
            let current = files.get(path).map(|(token, _)| token.to_string());
            if current.as_deref() != replacing {
                return Ok(Wrote::Conflict);
            }
            let mut made = self.made.lock().unwrap();
            *made += 1;
            files.insert(path.to_owned(), (*made + 100, bytes.to_vec()));
            Ok(Wrote::Written)
        }

        async fn remove(&self, path: &str, replacing: &str) -> Result<Wrote, StoreError> {
            if self.raced() {
                return Ok(Wrote::Conflict);
            }
            let mut files = self.files.lock().unwrap();
            match files.get(path) {
                Some((token, _)) if token.to_string() != replacing => Ok(Wrote::Conflict),
                _ => {
                    files.remove(path);
                    Ok(Wrote::Written)
                }
            }
        }
    }

    fn key() -> Key {
        Key::derive(&RouteId::new("r"), &Identity::new("i"))
    }

    #[test]
    fn the_store_passes_the_conformance_suite() {
        assert_eq!(
            pollster::block_on(store_conformance(&Kept::new(Memory::default()))),
            Ok(())
        );
    }

    #[test]
    fn each_write_of_a_record_counts_its_version_up_by_one() {
        let store = Kept::new(Memory::default());
        let entry = Entry::new(key());
        let versions: Vec<_> = [None, Some(1), Some(2)]
            .into_iter()
            .map(|expected| pollster::block_on(store.put_entry(&entry, expected.map(Version))))
            .collect();
        assert_eq!(
            versions,
            [1, 2, 3].map(|version| Ok(Written::Written(Version(version))))
        );
        let pin = pollster::block_on(store.put_pinned(&RouteId::new("r"), "1"));
        assert_eq!(pin, Ok(()));
        pollster::block_on(store.put_pinned(&RouteId::new("r"), "2")).unwrap();
        assert_eq!(
            pollster::block_on(store.pinned(&RouteId::new("r"))),
            Ok(Some("2".to_owned()))
        );
    }

    #[test]
    fn a_write_that_loses_a_race_to_an_unrelated_write_is_tried_again_and_then_given_up() {
        let entry = Entry::new(key());
        let twice = Kept::new(Memory::racing(2));
        assert_eq!(
            pollster::block_on(twice.put_entry(&entry, None)),
            Ok(Written::Written(Version(1)))
        );
        let thrice = Kept::new(Memory::racing(3));
        assert_eq!(
            pollster::block_on(thrice.put_entry(&entry, None)),
            Ok(Written::Conflict)
        );
        let pinning = Kept::new(Memory::racing(3));
        assert_eq!(
            pollster::block_on(pinning.put_pinned(&RouteId::new("r"), "1")),
            Err(StoreError::Unavailable {
                reason: "pins/r.json kept changing while herald wrote it".to_owned()
            })
        );
        let subscription = Subscription {
            destination: DestinationName::new("slack"),
            project: "p".to_owned(),
            subscriber: "u".to_owned(),
        };
        let removing = Kept::new(Memory::default());
        pollster::block_on(removing.subscribe(&subscription)).unwrap();
        *removing_races(&removing) = 3;
        assert_eq!(
            pollster::block_on(removing.unsubscribe(&subscription)),
            Err(StoreError::Unavailable {
                reason: "subscriptions/slack/p/u.json kept changing while herald removed it"
                    .to_owned()
            })
        );
        *removing_races(&removing) = 2;
        assert_eq!(
            pollster::block_on(removing.unsubscribe(&subscription)),
            Ok(())
        );
        assert_eq!(
            pollster::block_on(removing.subscribers(&DestinationName::new("slack"), "p")),
            Ok(Vec::new())
        );
    }

    fn removing_races(store: &Kept<Memory>) -> std::sync::MutexGuard<'_, u32> {
        store.records().races.lock().unwrap()
    }

    #[test]
    fn a_record_that_cannot_be_read_back_says_where_it_is() {
        let path = format!("ledger/{}.json", key());
        let garbled = Kept::new(Memory::holding(&path, b"not json"));
        let error = pollster::block_on(garbled.entry(&key())).unwrap_err();
        assert!(matches!(error, StoreError::Corrupt { .. }), "{error}");
        assert!(error.to_string().contains(&path), "{error}");
        let misshapen = Kept::new(Memory::holding(&path, br#"{"version":1,"record":7}"#));
        let error = pollster::block_on(misshapen.entry(&key())).unwrap_err();
        assert!(error.to_string().contains(&path), "{error}");
    }

    #[test]
    fn a_record_listed_but_gone_by_the_time_it_is_read_is_left_out() {
        let vanished = Memory {
            listed: vec!["schedules/gone.json".to_owned()],
            ..Memory::default()
        };
        assert_eq!(
            pollster::block_on(Kept::new(vanished).schedules()),
            Ok(Vec::new())
        );
    }
}
