//! The `sqlite` store's records in a real SQLite database, through the
//! conformance suite every store passes.

#[cfg(test)]
mod sqlite {
    use herald_native::NativeSqlite;
    use herald_ports::{Sql as _, Statement, StoreError, Value};
    use herald_records::{Dialect, Kept, Records as _, SqlRecords, Wrote};
    use herald_testing::{keyring_conformance, store_conformance};

    fn opened(sql: NativeSqlite) -> SqlRecords<NativeSqlite> {
        pollster::block_on(SqlRecords::open(sql, Dialect::Sqlite)).unwrap()
    }

    #[test]
    fn a_sqlite_database_keeps_every_record_a_store_keeps() {
        let store = Kept::new(opened(NativeSqlite::in_memory().unwrap()));
        pollster::block_on(store_conformance(&store)).unwrap();
        let keyring = Kept::new(opened(NativeSqlite::in_memory().unwrap()));
        pollster::block_on(keyring_conformance(&keyring)).unwrap();
    }

    #[test]
    fn a_record_is_replaced_only_as_it_was_read_and_never_takes_a_version_back() {
        let records = opened(NativeSqlite::in_memory().unwrap());
        let write = |bytes: &[u8], replacing: Option<&str>| {
            pollster::block_on(records.write("ledger/a.json", bytes, replacing)).unwrap()
        };
        let read = || pollster::block_on(records.read("ledger/a.json")).unwrap();
        assert_eq!(write(b"{}", None), Wrote::Written);
        assert_eq!(write(b"{}", None), Wrote::Conflict);
        let first = read().unwrap();
        assert_eq!(write(b"[1]", Some(&first.token)), Wrote::Written);
        assert_eq!(write(b"[2]", Some(&first.token)), Wrote::Conflict);
        assert_eq!(write(b"[2]", Some("not a version")), Wrote::Conflict);
        let second = read().unwrap();
        assert_eq!(second.bytes, b"[1]");
        assert_eq!(
            pollster::block_on(records.remove("ledger/a.json", &first.token)).unwrap(),
            Wrote::Conflict
        );
        assert_eq!(
            pollster::block_on(records.remove("ledger/a.json", &second.token)).unwrap(),
            Wrote::Written
        );
        assert_eq!(
            pollster::block_on(records.remove("ledger/a.json", &second.token)).unwrap(),
            Wrote::Written
        );
        assert_eq!(write(b"[3]", None), Wrote::Written);
        assert_eq!(write(b"[4]", Some(&second.token)), Wrote::Conflict);
        assert_eq!(
            pollster::block_on(records.write("ledger/b.json", &[0xff], None)),
            Err(StoreError::Corrupt {
                reason: "a record is not UTF-8 text".to_owned()
            })
        );
    }

    #[test]
    fn a_directory_lists_only_what_lies_beneath_it() {
        let records = opened(NativeSqlite::in_memory().unwrap());
        for path in [
            "subscriptions/slack/p/a.json",
            "subscriptions/slack/p/b.json",
            "subscriptions/slack/p0.json",
            "subscriptions/slack/p_/c.json",
            "subscriptions/slack/p.json",
        ] {
            pollster::block_on(records.write(path, b"{}", None)).unwrap();
        }
        assert_eq!(
            pollster::block_on(records.list("subscriptions/slack/p")).unwrap(),
            [
                "subscriptions/slack/p/a.json",
                "subscriptions/slack/p/b.json"
            ]
        );
        assert_eq!(
            pollster::block_on(records.list("cursors")).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_database_file_is_its_owner_s_alone_and_keeps_its_records_when_reopened() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("herald.sqlite");
        let records = opened(NativeSqlite::open(&path).unwrap());
        pollster::block_on(records.write("cursors/a.json", b"{}", None)).unwrap();
        drop(records);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let reopened = opened(NativeSqlite::open(&path).unwrap());
        assert_eq!(
            pollster::block_on(reopened.read("cursors/a.json"))
                .unwrap()
                .unwrap()
                .bytes,
            b"{}"
        );
        let missing = directory.path().join("missing/herald.sqlite");
        let Err(unmade) = NativeSqlite::open(&missing) else {
            panic!("a file in a missing directory cannot be made");
        };
        assert!(
            unmade
                .to_string()
                .contains(&format!("{} could not be made:", missing.display())),
            "{unmade}"
        );
    }

    #[test]
    fn a_database_another_herald_laid_out_is_refused() {
        let statement = |sql: &str| Statement {
            sql: sql.to_owned(),
            params: vec![],
        };
        let newer = NativeSqlite::in_memory().unwrap();
        pollster::block_on(newer.transaction(vec![
            statement(
                "CREATE TABLE herald_meta (id INTEGER PRIMARY KEY, layout BIGINT, made BIGINT)",
            ),
            statement("INSERT INTO herald_meta VALUES (1, 2, 0)"),
        ]))
        .unwrap();
        assert_eq!(
            pollster::block_on(SqlRecords::open(newer, Dialect::Sqlite)).err(),
            Some(StoreError::Corrupt {
                reason: "a newer herald laid the database out; run that herald or a later one"
                    .to_owned()
            })
        );
        let foreign = NativeSqlite::in_memory().unwrap();
        pollster::block_on(foreign.transaction(vec![
            statement(
                "CREATE TABLE herald_meta (id INTEGER PRIMARY KEY, layout TEXT, made BIGINT)",
            ),
            statement("INSERT INTO herald_meta VALUES (1, 'x', 0)"),
        ]))
        .unwrap();
        assert_eq!(
            pollster::block_on(SqlRecords::open(foreign, Dialect::Sqlite)).err(),
            Some(StoreError::Corrupt {
                reason: "the database's herald_meta table is not herald's".to_owned()
            })
        );
    }

    #[test]
    fn each_statement_answers_its_rows_and_the_rows_it_changed() {
        let sql = NativeSqlite::in_memory().unwrap();
        let statement = |sql: &str, params: Vec<Value>| Statement {
            sql: sql.to_owned(),
            params,
        };
        let answers = pollster::block_on(sql.transaction(vec![
            statement("CREATE TABLE t (a INTEGER, b TEXT)", vec![]),
            statement(
                "INSERT INTO t VALUES (?1, ?2), (?1, ?3)",
                vec![Value::Integer(7), Value::Text("x".to_owned()), Value::Null],
            ),
            statement("CREATE INDEX t_a ON t (a)", vec![]),
            statement("SELECT a, b FROM t ORDER BY b", vec![]),
        ]))
        .unwrap();
        assert_eq!(answers[1].changed, 2);
        assert_eq!(answers[2].changed, 0);
        assert_eq!(
            answers[3].rows,
            [
                vec![Value::Integer(7), Value::Null],
                vec![Value::Integer(7), Value::Text("x".to_owned())],
            ]
        );
        let refused = pollster::block_on(sql.transaction(vec![
            statement("INSERT INTO t VALUES (1, 'kept?')", vec![]),
            statement("SELECT 1.5", vec![]),
        ]));
        assert!(refused.is_err());
        let count =
            pollster::block_on(sql.transaction(vec![statement("SELECT count(*) FROM t", vec![])]))
                .unwrap();
        assert_eq!(count[0].rows, [[Value::Integer(2)]]);
        assert!(pollster::block_on(sql.transaction(vec![statement("NOT SQL", vec![])])).is_err());
    }
}
