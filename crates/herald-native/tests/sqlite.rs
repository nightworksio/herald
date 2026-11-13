//! The `sqlite` store's records in a real SQLite database, through the
//! conformance suite every store passes.

#[cfg(test)]
mod sqlite {
    use herald_native::{NativeSql, NativeSqlite};
    use herald_ports::{Sql as _, SqlError, Statement, StoreError, Value};
    use herald_records::{Dialect, RecordBackend as _, RecordStore, SqlRecords, Wrote};
    use herald_testing::{keyring_conformance, sql_conformance, store_conformance};
    use jiff::SignedDuration;

    const BUSY: SignedDuration = SignedDuration::from_secs(5);

    fn run<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn opened(sql: NativeSqlite) -> SqlRecords<NativeSqlite> {
        run(SqlRecords::open(sql, Dialect::Sqlite)).unwrap()
    }

    #[test]
    fn a_sqlite_database_keeps_every_record_a_store_keeps() {
        let store = RecordStore::new(opened(NativeSqlite::in_memory().unwrap()));
        run(store_conformance(&store)).unwrap();
        let keyring = RecordStore::new(opened(NativeSqlite::in_memory().unwrap()));
        run(keyring_conformance(&keyring)).unwrap();
    }

    #[test]
    fn a_sqlite_database_and_the_native_database_over_one_pass_the_database_suite() {
        let parameters = Dialect::Sqlite.parameters();
        let file = tempfile::tempdir().unwrap();
        let opened = NativeSqlite::open(&file.path().join("herald.sqlite"), BUSY).unwrap();
        run(sql_conformance(&opened, parameters)).unwrap();
        run(sql_conformance(
            &NativeSqlite::in_memory().unwrap(),
            parameters,
        ))
        .unwrap();
        let native = NativeSql::Sqlite(NativeSqlite::in_memory().unwrap());
        run(sql_conformance(&native, parameters)).unwrap();
    }

    #[test]
    fn the_native_database_keeps_a_sqlite_file_s_records() {
        let sql = NativeSql::Sqlite(NativeSqlite::in_memory().unwrap());
        let store = run(SqlRecords::kept(sql, Dialect::Sqlite, None)).unwrap();
        run(store_conformance(&store)).unwrap();
    }

    #[test]
    fn a_record_is_replaced_only_as_it_was_read_and_never_takes_a_version_back() {
        let records = opened(NativeSqlite::in_memory().unwrap());
        let write = |bytes: &[u8], replacing: Option<&str>| {
            run(records.write("ledger/a.json", bytes, replacing)).unwrap()
        };
        let read = || run(records.read("ledger/a.json")).unwrap();
        assert_eq!(write(b"{}", None), Wrote::Written);
        assert_eq!(write(b"{}", None), Wrote::Conflict);
        let first = read().unwrap();
        assert_eq!(write(b"[1]", Some(&first.token)), Wrote::Written);
        assert_eq!(write(b"[2]", Some(&first.token)), Wrote::Conflict);
        assert_eq!(write(b"[2]", Some("not a version")), Wrote::Conflict);
        let second = read().unwrap();
        assert_eq!(second.bytes, b"[1]");
        assert_eq!(
            run(records.remove("ledger/a.json", &first.token)).unwrap(),
            Wrote::Conflict
        );
        assert_eq!(
            run(records.remove("ledger/a.json", &second.token)).unwrap(),
            Wrote::Written
        );
        assert_eq!(
            run(records.remove("ledger/a.json", &second.token)).unwrap(),
            Wrote::Written
        );
        assert_eq!(write(b"[3]", None), Wrote::Written);
        assert_eq!(write(b"[4]", Some(&first.token)), Wrote::Conflict);
        assert_eq!(write(b"[4]", Some(&second.token)), Wrote::Conflict);
        assert_eq!(
            run(records.write("ledger/b.json", &[0xff], None)),
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
            run(records.write(path, b"{}", None)).unwrap();
        }
        assert_eq!(
            run(records.list("subscriptions/slack/p")).unwrap(),
            [
                "subscriptions/slack/p/a.json",
                "subscriptions/slack/p/b.json"
            ]
        );
        assert_eq!(run(records.list("cursors")).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_database_file_is_its_owner_s_alone_and_keeps_its_records_when_reopened() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("herald.sqlite");
        let records = opened(NativeSqlite::open(&path, BUSY).unwrap());
        run(records.write("cursors/a.json", b"{}", None)).unwrap();
        drop(records);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let reopened = opened(NativeSqlite::open(&path, BUSY).unwrap());
        assert_eq!(
            run(reopened.read("cursors/a.json")).unwrap().unwrap().bytes,
            b"{}"
        );
        let missing = directory.path().join("missing/herald.sqlite");
        let Err(unmade) = NativeSqlite::open(&missing, BUSY) else {
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
    fn a_transaction_waits_for_the_lock_no_longer_than_it_was_given_and_holds_up_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("herald.sqlite");
        let sql = NativeSqlite::open(&path, SignedDuration::from_millis(600)).unwrap();
        let holder = rusqlite::Connection::open(&path).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let statement = Statement::change("CREATE TABLE t (a INTEGER)", vec![]);
        let started = std::time::Instant::now();
        let (waited, ticked) = run(async {
            tokio::join!(sql.transaction(vec![statement.clone()]), async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                started.elapsed()
            })
        });
        let waited_for = started.elapsed();
        assert!(waited.unwrap_err().to_string().contains("locked"));
        assert!(
            waited_for >= std::time::Duration::from_millis(600),
            "{waited_for:?}"
        );
        assert!(ticked < std::time::Duration::from_millis(500), "{ticked:?}");
        holder.execute_batch("COMMIT").unwrap();
        assert!(run(sql.transaction(vec![statement])).is_ok());
    }

    #[test]
    fn a_database_another_herald_laid_out_is_refused() {
        let statement = |sql: &str| Statement::change(sql, vec![]);
        let newer = NativeSqlite::in_memory().unwrap();
        run(newer.transaction(vec![
            statement(
                "CREATE TABLE herald_meta (id INTEGER PRIMARY KEY, layout BIGINT, made BIGINT)",
            ),
            statement("INSERT INTO herald_meta VALUES (1, 3, 0)"),
        ]))
        .unwrap();
        assert_eq!(
            run(SqlRecords::open(newer, Dialect::Sqlite)).err(),
            Some(StoreError::Corrupt {
                reason: "a newer herald laid the database out; run that herald or a later one"
                    .to_owned()
            })
        );
        let foreign = NativeSqlite::in_memory().unwrap();
        run(foreign.transaction(vec![
            statement(
                "CREATE TABLE herald_meta (id INTEGER PRIMARY KEY, layout TEXT, made BIGINT)",
            ),
            statement("INSERT INTO herald_meta VALUES (1, 'x', 0)"),
        ]))
        .unwrap();
        assert_eq!(
            run(SqlRecords::open(foreign, Dialect::Sqlite)).err(),
            Some(StoreError::Corrupt {
                reason: "the database's herald_meta table is not herald's".to_owned()
            })
        );
    }

    #[test]
    fn each_statement_answers_its_rows_and_the_rows_it_changed() {
        let sql = NativeSqlite::in_memory().unwrap();
        let statement = |sql: &str, params: Vec<Value>| {
            if sql.starts_with("SELECT") {
                Statement::query(sql, params)
            } else {
                Statement::change(sql, params)
            }
        };
        let answers = run(sql.transaction(vec![
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
        let refused = run(sql.transaction(vec![
            statement("INSERT INTO t VALUES (1, 'kept?')", vec![]),
            statement("SELECT 1.5", vec![]),
        ]));
        assert_eq!(
            refused,
            Err(SqlError::Unavailable {
                reason: "a column holds neither an integer nor text".to_owned()
            })
        );
        let count =
            run(sql.transaction(vec![statement("SELECT count(*) FROM t", vec![])])).unwrap();
        assert_eq!(count[0].rows, [[Value::Integer(2)]]);
        assert_eq!(
            run(sql.transaction(vec![statement("NOT SQL", vec![])])),
            Err(SqlError::Unavailable {
                reason: "near \"NOT\": syntax error in NOT SQL at offset 0".to_owned()
            })
        );
    }
}
