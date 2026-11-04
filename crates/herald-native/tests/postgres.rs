//! The `postgres` store's records in a real Postgres database, through the
//! conformance suite every store passes. CI hands its Postgres service in
//! `HERALD_TEST_POSTGRES`; elsewhere these tests run only where it is set.

#[cfg(test)]
mod postgres {
    use herald_native::NativePostgres;
    use herald_ports::{Secret, Sql as _, SqlError, Statement, StoreError, Value};
    use herald_records::{Dialect, Kept, Records as _, SqlRecords, Wrote};
    use herald_testing::store_conformance;

    const URL: &str = "HERALD_TEST_POSTGRES";

    fn url() -> Option<String> {
        let url = std::env::var(URL).ok().filter(|url| !url.is_empty());
        assert!(
            url.is_some() || std::env::var_os("CI").is_none(),
            "{URL} is not set, and CI runs these tests against its Postgres service"
        );
        url
    }

    fn statement(sql: &str, params: Vec<Value>) -> Statement {
        Statement {
            sql: sql.to_owned(),
            params,
        }
    }

    async fn fresh(schema: &str) -> Option<NativePostgres> {
        let sql = NativePostgres::connect(&Secret::new(URL, url()?))
            .await
            .unwrap();
        sql.transaction(vec![
            statement(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"), vec![]),
            statement(&format!("CREATE SCHEMA {schema}"), vec![]),
            statement(&format!("SET search_path TO {schema}"), vec![]),
        ])
        .await
        .unwrap();
        Some(sql)
    }

    async fn opened(schema: &str) -> Option<SqlRecords<NativePostgres>> {
        Some(
            SqlRecords::open(fresh(schema).await?, Dialect::Postgres)
                .await
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn a_postgres_database_keeps_every_record_a_store_keeps() {
        let Some(records) = opened("conformance").await else {
            return;
        };
        store_conformance(&Kept::new(records)).await.unwrap();
    }

    #[tokio::test]
    async fn a_record_is_replaced_only_as_it_was_read_and_never_takes_a_version_back() {
        let Some(records) = opened("versions").await else {
            return;
        };
        assert_eq!(
            records.write("ledger/a.json", b"{}", None).await,
            Ok(Wrote::Written)
        );
        assert_eq!(
            records.write("ledger/a.json", b"{}", None).await,
            Ok(Wrote::Conflict)
        );
        let first = records.read("ledger/a.json").await.unwrap().unwrap();
        assert_eq!(
            records
                .write("ledger/a.json", b"[1]", Some(&first.token))
                .await,
            Ok(Wrote::Written)
        );
        assert_eq!(
            records
                .write("ledger/a.json", b"[2]", Some(&first.token))
                .await,
            Ok(Wrote::Conflict)
        );
        let second = records.read("ledger/a.json").await.unwrap().unwrap();
        assert_eq!(second.bytes, b"[1]");
        assert_eq!(
            records.remove("ledger/a.json", &first.token).await,
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            records.remove("ledger/a.json", &second.token).await,
            Ok(Wrote::Written)
        );
        assert_eq!(
            records.remove("ledger/a.json", &second.token).await,
            Ok(Wrote::Written)
        );
        assert_eq!(
            records.write("ledger/a.json", b"[3]", None).await,
            Ok(Wrote::Written)
        );
        assert_eq!(
            records
                .write("ledger/a.json", b"[4]", Some(&second.token))
                .await,
            Ok(Wrote::Conflict)
        );
    }

    #[tokio::test]
    async fn a_directory_lists_only_what_lies_beneath_it_in_byte_order() {
        let Some(records) = opened("listing").await else {
            return;
        };
        for path in [
            "subscriptions/slack/p/b.json",
            "subscriptions/slack/p/A.json",
            "subscriptions/slack/p0.json",
            "subscriptions/slack/p_/c.json",
            "subscriptions/slack/p.json",
            "subscriptions/slack/p-q/d.json",
        ] {
            records.write(path, b"{}", None).await.unwrap();
        }
        assert_eq!(
            records.list("subscriptions/slack/p").await.unwrap(),
            [
                "subscriptions/slack/p/A.json",
                "subscriptions/slack/p/b.json"
            ]
        );
    }

    #[tokio::test]
    async fn a_database_a_newer_herald_laid_out_is_refused() {
        let Some(sql) = fresh("newer").await else {
            return;
        };
        sql.transaction(vec![
            statement(
                "CREATE TABLE herald_meta (id INTEGER PRIMARY KEY, layout BIGINT, made BIGINT)",
                vec![],
            ),
            statement("INSERT INTO herald_meta VALUES (1, 2, 0)", vec![]),
        ])
        .await
        .unwrap();
        assert_eq!(
            SqlRecords::open(sql, Dialect::Postgres).await.err(),
            Some(StoreError::Corrupt {
                reason: "a newer herald laid the database out; run that herald or a later one"
                    .to_owned()
            })
        );
    }

    #[tokio::test]
    async fn each_statement_answers_its_rows_and_the_rows_it_changed() {
        let Some(sql) = fresh("answers").await else {
            return;
        };
        let answers = sql
            .transaction(vec![
                statement("CREATE TABLE t (a BIGINT, b TEXT, c INTEGER)", vec![]),
                statement(
                    "INSERT INTO t VALUES ($1, $2, 3), ($1, $3, NULL)",
                    vec![Value::Integer(7), Value::Text("x".to_owned()), Value::Null],
                ),
                statement("SELECT a, b, c FROM t ORDER BY b", vec![]),
            ])
            .await
            .unwrap();
        assert_eq!(answers[1].changed, 2);
        assert_eq!(
            answers[2].rows,
            [
                vec![
                    Value::Integer(7),
                    Value::Text("x".to_owned()),
                    Value::Integer(3)
                ],
                vec![Value::Integer(7), Value::Null, Value::Null],
            ]
        );
        let refused = sql
            .transaction(vec![
                statement("INSERT INTO t VALUES (1, 'kept?', 1)", vec![]),
                statement("SELECT 1.5::float8", vec![]),
            ])
            .await;
        assert_eq!(
            refused,
            Err(SqlError::Unavailable {
                reason: "a column holds neither an integer nor text".to_owned()
            })
        );
        let count = sql
            .transaction(vec![statement("SELECT count(*) FROM t", vec![])])
            .await
            .unwrap();
        assert_eq!(count[0].rows, [[Value::Integer(2)]]);
        let Err(SqlError::Unavailable { reason }) =
            sql.transaction(vec![statement("NOT SQL", vec![])]).await
        else {
            panic!("a statement that is no SQL is refused");
        };
        assert!(reason.contains("syntax error"), "{reason}");
    }

    #[tokio::test]
    async fn a_url_that_does_not_disable_tls_is_reached_over_tls_alone() {
        let Some(url) = url() else {
            return;
        };
        let base = url.replace("sslmode=disable", "").replace("?&", "?");
        let base = base.trim_end_matches(['?', '&']);
        for plain in [
            base.to_owned(),
            format!("{base}?sslmode=prefer"),
            format!("{base}?application_name=sslmode"),
        ] {
            let Err(SqlError::Unavailable { reason }) =
                NativePostgres::connect(&Secret::new(URL, plain.as_str())).await
            else {
                panic!("a server without TLS is refused for {plain}");
            };
            assert!(
                reason.contains("TLS") || reason.contains("tls") || reason.contains("SSL"),
                "{plain}: {reason}"
            );
        }
        assert_eq!(
            NativePostgres::connect(&Secret::new(URL, "not a url"))
                .await
                .err(),
            Some(SqlError::Unavailable {
                reason: format!("the secret {URL} is not a Postgres connection URL")
            })
        );
    }
}
