//! The records a dry run keeps in memory, through the conformance suite
//! every store passes and the contract every backend keeps.

#[cfg(test)]
mod memory {
    use herald_ports::{Keyring, KnownKey};
    use herald_records::{Kept, Memory, MemoryKeyring, Records, Wrote};
    use herald_testing::{keyring_conformance, store_conformance};
    use jiff::Timestamp;

    #[test]
    fn memory_passes_the_conformance_suite() {
        assert_eq!(
            pollster::block_on(store_conformance(&Kept::new(Memory::default()))),
            Ok(())
        );
    }

    #[test]
    fn memory_and_the_store_kept_in_it_pass_the_keyring_suite() {
        assert_eq!(
            pollster::block_on(keyring_conformance(&MemoryKeyring::new(8))),
            Ok(())
        );
        assert_eq!(
            pollster::block_on(keyring_conformance(&Kept::new(Memory::default()))),
            Ok(())
        );
    }

    fn known(pem: &str) -> KnownKey {
        KnownKey {
            owner: "https://social.example/users/ana".to_owned(),
            pem: pem.to_owned(),
            fetched: Timestamp::UNIX_EPOCH,
        }
    }

    #[test]
    fn a_full_keyring_forgets_the_key_used_longest_ago() {
        let keyring = MemoryKeyring::new(2);
        pollster::block_on(keyring.know("a", &known("a"))).unwrap();
        pollster::block_on(keyring.know("b", &known("b"))).unwrap();
        assert!(pollster::block_on(keyring.known("a")).unwrap().is_some());
        pollster::block_on(keyring.know("c", &known("c"))).unwrap();
        assert!(pollster::block_on(keyring.known("b")).unwrap().is_none());
        assert_eq!(
            pollster::block_on(keyring.known("a")).unwrap(),
            Some(known("a"))
        );
        assert_eq!(
            pollster::block_on(keyring.known("c")).unwrap(),
            Some(known("c"))
        );
        pollster::block_on(keyring.know("a", &known("a2"))).unwrap();
        assert_eq!(
            pollster::block_on(keyring.known("c")).unwrap(),
            Some(known("c"))
        );
        assert_eq!(
            pollster::block_on(keyring.known("a")).unwrap(),
            Some(known("a2"))
        );
    }

    #[test]
    fn the_key_known_longest_ago_is_forgotten_whatever_its_name() {
        let keyring = MemoryKeyring::new(2);
        pollster::block_on(keyring.know("b", &known("b"))).unwrap();
        pollster::block_on(keyring.know("a", &known("a"))).unwrap();
        pollster::block_on(keyring.know("c", &known("c"))).unwrap();
        assert!(pollster::block_on(keyring.known("b")).unwrap().is_none());
        assert_eq!(
            pollster::block_on(keyring.known("a")).unwrap(),
            Some(known("a"))
        );
    }

    #[test]
    fn a_keyring_holding_no_keys_keeps_none() {
        let keyring = MemoryKeyring::new(0);
        pollster::block_on(keyring.know("a", &known("a"))).unwrap();
        assert!(pollster::block_on(keyring.known("a")).unwrap().is_none());
    }

    #[test]
    fn a_write_or_removal_naming_a_record_since_replaced_changes_nothing() {
        let memory = Memory::default();
        assert_eq!(
            pollster::block_on(memory.remove("p/a", "1")),
            Ok(Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(memory.write("p/a", b"one", None)),
            Ok(Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(memory.write("p/a", b"two", Some("9"))),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(memory.remove("p/a", "9")),
            Ok(Wrote::Conflict)
        );
        let read = pollster::block_on(memory.read("p/a")).unwrap().unwrap();
        assert_eq!(
            (read.token.as_str(), read.bytes.as_slice()),
            ("1", &b"one"[..])
        );
        assert_eq!(
            pollster::block_on(memory.list("p")),
            Ok(vec!["p/a".to_owned()])
        );
        assert_eq!(
            pollster::block_on(memory.remove("p/a", "1")),
            Ok(Wrote::Written)
        );
        assert_eq!(pollster::block_on(memory.read("p/a")), Ok(None));
    }
}
