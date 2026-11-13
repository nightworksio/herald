//! The database and store conformance suites over Spin's own databases,
//! built only with the `conformance` feature, for testing a host:
//! `/conformance/sqlite` runs them over Spin's SQLite database `default`,
//! `/conformance/postgres` over the Postgres database `HERALD_TEST_POSTGRES`
//! names.

use herald_core::status;
use herald_flows::Reply;
use herald_ports::{Parameters, Secret, Sql as _, SqlError, Statement};
use herald_records::{Dialect, SqlRecords};
use herald_testing::{sql_conformance, store_conformance};
use jiff::SignedDuration;

use super::spin::SpinSql;

const URL: &str = "HERALD_TEST_POSTGRES";

fn plain(status: u16, body: impl Into<String>) -> Reply {
    Reply {
        status,
        kind: "text/plain; charset=utf-8",
        body: body.into().into_bytes(),
        retry_after: None,
    }
}

/// A failing statement's error, which a rollback must not hide.
async fn failure_kept(sql: &SpinSql) -> Result<(), String> {
    match sql
        .transaction(vec![Statement::change("NOT SQL", vec![])])
        .await
    {
        Err(SqlError::Unavailable { reason }) if reason.contains("syntax") => Ok(()),
        answered => Err(format!("a statement that is not SQL answered {answered:?}")),
    }
}

async fn conforms(sql: SpinSql, postgres: bool) -> Result<(), String> {
    failure_kept(&sql).await?;
    let parameters = if postgres {
        Parameters::Dollar
    } else {
        Parameters::Question
    };
    sql_conformance(&sql, parameters)
        .await
        .map_err(|failure| failure.to_string())?;
    if postgres {
        let store = SqlRecords::kept(sql, Dialect::Postgres, None)
            .await
            .map_err(|error| error.to_string())?;
        store_conformance(&store).await
    } else {
        let store = SqlRecords::kept(sql, Dialect::Sqlite, None)
            .await
            .map_err(|error| error.to_string())?;
        store_conformance(&store).await
    }
    .map_err(|failure| failure.to_string())
}

/// The suite's answer for `path`, or `None` where it names none.
pub(super) async fn answered(path: &str) -> Option<Reply> {
    let sql = match path {
        "/conformance/sqlite" => SpinSql::Sqlite {
            label: "default".to_owned(),
            busy: SignedDuration::from_secs(5),
        },
        "/conformance/postgres" => match std::env::var(URL) {
            Ok(url) => SpinSql::Postgres(Secret::new(URL, url)),
            Err(_) => return Some(plain(status::SERVER_ERROR, format!("{URL} is not set"))),
        },
        _ => return None,
    };
    let postgres = matches!(sql, SpinSql::Postgres(_));
    Some(match conforms(sql, postgres).await {
        Ok(()) => plain(status::OK, "conforms"),
        Err(failure) => plain(status::SERVER_ERROR, failure),
    })
}
