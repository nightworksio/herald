//! The records a key-value store keeps, through the conformance suite every
//! store passes, and through a store that is down or changed underneath.

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the fakes answer at once, and are written as the async trait they fake"
)]
mod kv {
    use herald_ports::{Kv, KvError, Pair, Stamped, StoreError, Versionstamp};
    use herald_records::{Kept, KvRecords, Records, Wrote};
    use herald_testing::{FakeKv, keyring_conformance, store_conformance};

    #[test]
    fn a_key_value_store_passes_the_conformance_suites() {
        assert_eq!(
            pollster::block_on(store_conformance(&Kept::new(KvRecords::new(
                FakeKv::default(),
                "herald"
            )))),
            Ok(())
        );
        assert_eq!(
            pollster::block_on(keyring_conformance(&Kept::new(KvRecords::new(
                FakeKv::default(),
                "/apps/herald/"
            )))),
            Ok(())
        );
    }

    #[test]
    fn records_are_kept_under_the_prefix_and_listed_below_their_directory() {
        let records = KvRecords::new(FakeKv::default(), "apps/herald");
        for path in ["ledger/a", "ledger/b/c", "ledgers/d"] {
            assert_eq!(
                pollster::block_on(records.write(path, b"{}", None)),
                Ok(Wrote::Written)
            );
        }
        assert_eq!(
            pollster::block_on(records.list("ledger")),
            Ok(vec!["ledger/a".to_owned(), "ledger/b/c".to_owned()])
        );
        let read = pollster::block_on(records.read("ledger/a")).unwrap().unwrap();
        assert_eq!(read.bytes, b"{}");
        assert_eq!(
            pollster::block_on(records.write("ledger/a", b"[]", Some("stale"))),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.remove("ledger/a", "stale")),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.remove("ledger/a", &read.token)),
            Ok(Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(records.remove("ledger/a", &read.token)),
            Ok(Wrote::Written)
        );
        assert_eq!(pollster::block_on(records.read("ledger/a")), Ok(None));
    }

    #[derive(Debug)]
    struct Down;

    fn down() -> KvError {
        KvError::Unavailable {
            reason: "down".to_owned(),
        }
    }

    impl Kv for Down {
        async fn get(&self, _: &[String]) -> Result<Option<Stamped>, KvError> {
            Err(down())
        }

        async fn set_if(
            &self,
            _: &[String],
            _: Vec<u8>,
            _: Option<Versionstamp>,
        ) -> Result<Option<Versionstamp>, KvError> {
            Err(down())
        }

        async fn delete_if(&self, _: &[String], _: Versionstamp) -> Result<bool, KvError> {
            Ok(false)
        }

        async fn list(&self, _: &[String]) -> Result<Vec<Pair>, KvError> {
            Err(down())
        }
    }

    #[derive(Debug)]
    struct Refusing;

    impl Kv for Refusing {
        async fn get(&self, _: &[String]) -> Result<Option<Stamped>, KvError> {
            Ok(None)
        }

        async fn set_if(
            &self,
            _: &[String],
            _: Vec<u8>,
            _: Option<Versionstamp>,
        ) -> Result<Option<Versionstamp>, KvError> {
            Ok(None)
        }

        async fn delete_if(&self, _: &[String], _: Versionstamp) -> Result<bool, KvError> {
            Err(down())
        }

        async fn list(&self, _: &[String]) -> Result<Vec<Pair>, KvError> {
            Ok(vec![Pair {
                key: Vec::new(),
                value: Vec::new(),
            }])
        }
    }

    fn unavailable<T>() -> Result<T, StoreError> {
        Err(StoreError::Unavailable {
            reason: "the key-value store could not be reached: down".to_owned(),
        })
    }

    #[test]
    fn a_store_that_is_down_says_so_for_every_record() {
        let records = KvRecords::new(Down, "herald");
        assert_eq!(pollster::block_on(records.read("a")), unavailable());
        assert_eq!(pollster::block_on(records.list("a")), unavailable());
        assert_eq!(
            pollster::block_on(records.write("a", b"", None)),
            unavailable()
        );
        assert_eq!(pollster::block_on(records.remove("a", "1")), unavailable());
        let refusing = KvRecords::new(Refusing, "herald");
        assert_eq!(pollster::block_on(refusing.remove("a", "1")), unavailable());
        assert_eq!(pollster::block_on(refusing.list("a")), Ok(Vec::new()));
    }
}
