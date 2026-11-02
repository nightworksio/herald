//! The records a dry run keeps in memory, through the conformance suite
//! every store passes and the contract every backend keeps.

#[cfg(test)]
mod memory {
    use herald_records::{Kept, Memory, Records, Wrote};
    use herald_testing::store_conformance;

    #[test]
    fn memory_passes_the_conformance_suite() {
        assert_eq!(
            pollster::block_on(store_conformance(&Kept::new(Memory::default()))),
            Ok(())
        );
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
