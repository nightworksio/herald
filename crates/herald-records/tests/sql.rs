//! Records kept in a SQL database: the statements each dialect is sent,
//! and every answer a database could give read as a store must read it.

#[cfg(test)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "each helper is a scripted answer, which a database gives as a Result"
)]
mod sql {
    use herald_ports::{Changed, SqlError, StoreError, Value};
    use herald_records::{Dialect, LAYOUT, Records as _, SqlRecords, Wrote};
    use herald_testing::ScriptedSql;

    fn laid_out(layout: Value) -> Result<Vec<Changed>, SqlError> {
        Ok(vec![
            Changed::default(),
            Changed::default(),
            Changed::default(),
            Changed {
                rows: vec![vec![layout]],
                changed: 0,
            },
        ])
    }

    fn rows(rows: Vec<Vec<Value>>) -> Result<Vec<Changed>, SqlError> {
        Ok(vec![Changed { rows, changed: 0 }])
    }

    fn changed(counts: &[u64]) -> Result<Vec<Changed>, SqlError> {
        Ok(counts
            .iter()
            .map(|&changed| Changed {
                rows: vec![],
                changed,
            })
            .collect())
    }

    fn opened(
        dialect: Dialect,
        answers: Vec<Result<Vec<Changed>, SqlError>>,
    ) -> SqlRecords<ScriptedSql> {
        let sql = answers.into_iter().fold(
            ScriptedSql::default().answering(laid_out(Value::Integer(LAYOUT))),
            ScriptedSql::answering,
        );
        pollster::block_on(SqlRecords::open(sql, dialect)).unwrap()
    }

    fn corrupt(reason: &str) -> StoreError {
        StoreError::Corrupt {
            reason: reason.to_owned(),
        }
    }

    fn text(value: &str) -> Value {
        Value::Text(value.to_owned())
    }

    #[test]
    fn postgres_is_sent_its_own_numbered_parameters() {
        let sql = ScriptedSql::default()
            .answering(laid_out(Value::Integer(LAYOUT)))
            .answering(rows(vec![vec![
                text("{}"),
                text("0f3a"),
                Value::Integer(4),
            ]]))
            .answering(changed(&[1]));
        let records = pollster::block_on(SqlRecords::open(sql, Dialect::Postgres)).unwrap();
        let blob = pollster::block_on(records.read("ledger/a.json"))
            .unwrap()
            .unwrap();
        assert_eq!(
            (blob.token.as_str(), blob.bytes.as_slice()),
            ("0f3a.4", b"{}".as_slice())
        );
        assert_eq!(
            pollster::block_on(records.write("ledger/b.json", b"{}", None)),
            Ok(Wrote::Written)
        );
        let debug = format!("{records:?}");
        assert!(
            debug.contains("SELECT body, epoch, version FROM herald_records WHERE path = $1"),
            "{debug}"
        );
        assert!(
            debug.contains("SELECT 1, $1 WHERE NOT EXISTS (SELECT 1 FROM herald_meta) ON CONFLICT (id) DO NOTHING"),
            "{debug}"
        );
        assert!(
            debug.contains("VALUES ($1, $2, replace(gen_random_uuid()::text, '-', ''), 1)"),
            "{debug}"
        );
        assert!(!debug.contains("?1"), "{debug}");
        assert!(
            debug.contains("path TEXT COLLATE \\\"C\\\" PRIMARY KEY"),
            "{debug}"
        );
    }

    #[test]
    fn a_layout_that_is_not_herald_s_is_refused_as_it_opens() {
        for layout in [Value::Text("1".to_owned()), Value::Integer(0)] {
            let sql = ScriptedSql::default().answering(laid_out(layout));
            assert_eq!(
                pollster::block_on(SqlRecords::open(sql, Dialect::Sqlite)).err(),
                Some(corrupt("the database's herald_meta table is not herald's"))
            );
        }
        let preview = ScriptedSql::default().answering(laid_out(Value::Integer(LAYOUT - 1)));
        assert_eq!(
            pollster::block_on(SqlRecords::open(preview, Dialect::Sqlite)).err(),
            Some(corrupt(
                "a preview of herald laid the database out; give this herald an empty database"
            ))
        );
        let newer = ScriptedSql::default().answering(laid_out(Value::Integer(LAYOUT + 1)));
        assert_eq!(
            pollster::block_on(SqlRecords::open(newer, Dialect::Sqlite)).err(),
            Some(corrupt(
                "a newer herald laid the database out; run that herald or a later one"
            ))
        );
        let unlaid = ScriptedSql::default().answering(changed(&[0, 0, 0]));
        assert_eq!(
            pollster::block_on(SqlRecords::open(unlaid, Dialect::Sqlite)).err(),
            Some(corrupt("the database's herald_meta table is not herald's"))
        );
    }

    #[test]
    fn a_row_that_is_not_a_record_is_read_as_corrupt() {
        let records = opened(
            Dialect::Sqlite,
            vec![
                rows(vec![]),
                rows(vec![vec![text("{}"), text("e"), text("4")]]),
                rows(vec![
                    vec![text("{}"), text("e"), Value::Integer(1)],
                    vec![text("{}"), text("e"), Value::Integer(2)],
                ]),
                rows(vec![vec![Value::Integer(1)]]),
                rows(vec![
                    vec![text("ledger/a.json")],
                    vec![text("ledger/b.json")],
                ]),
                Err(SqlError::Unavailable {
                    reason: "locked".to_owned(),
                }),
            ],
        );
        let read = || pollster::block_on(records.read("ledger/a.json"));
        assert_eq!(read(), Ok(None));
        assert_eq!(
            read(),
            Err(corrupt(
                "a record's row is not a body, an epoch and a version"
            ))
        );
        assert_eq!(read(), Err(corrupt("a path holds more than one record")));
        assert_eq!(
            pollster::block_on(records.list("ledger")),
            Err(corrupt("a record's path is not text"))
        );
        assert_eq!(
            pollster::block_on(records.list("ledger")),
            Ok(vec!["ledger/a.json".to_owned(), "ledger/b.json".to_owned()])
        );
        assert_eq!(
            read(),
            Err(StoreError::Unavailable {
                reason: "the database refused the statements: locked".to_owned()
            })
        );
    }

    #[test]
    fn a_write_or_removal_reads_what_the_database_changed() {
        let records = opened(
            Dialect::Sqlite,
            vec![
                changed(&[1]),
                changed(&[0]),
                changed(&[1]),
                Ok(vec![
                    Changed::default(),
                    Changed {
                        rows: vec![vec![Value::Integer(9)]],
                        changed: 0,
                    },
                ]),
                changed(&[1, 0]),
                changed(&[0, 0]),
            ],
        );
        let write =
            |replacing: Option<&str>| pollster::block_on(records.write("a/b", b"{}", replacing));
        assert_eq!(write(Some("e.3")), Ok(Wrote::Written));
        assert_eq!(write(None), Ok(Wrote::Conflict));
        for unparsed in ["3", "e.x", "e"] {
            assert_eq!(write(Some(unparsed)), Ok(Wrote::Conflict), "{unparsed}");
            assert_eq!(
                pollster::block_on(records.remove("a/b", unparsed)),
                Ok(Wrote::Conflict),
                "{unparsed}"
            );
        }
        assert_eq!(write(None), Ok(Wrote::Written));
        assert_eq!(
            pollster::block_on(records.remove("a/b", "e.3")),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.remove("a/b", "e.3")),
            Ok(Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(records.remove("a/b", "e.3")),
            Ok(Wrote::Written)
        );
        let debug = format!("{records:?}");
        assert!(
            debug.contains("VALUES (?1, ?2, lower(hex(randomblob(16))), 1)"),
            "{debug}"
        );
        assert!(
            debug.contains("SET body = ?2, version = version + 1 WHERE path = ?1 AND epoch = ?3 AND version = ?4"),
            "{debug}"
        );
    }
}
