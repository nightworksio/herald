use herald_ports::{Changed, Secret, Sql, SqlError, Statement, Value};
use tokio::sync::Mutex;
use tokio_postgres::config::SslMode;
use tokio_postgres::types::{ToSql, Type};
use tokio_postgres::{Client, Config, Row};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::http::{chain, tls};

/// A Postgres database, through which the `postgres` store keeps its records
/// (ADR-0005). The connection is TLS unless its URL sets `sslmode=disable`:
/// `prefer`, which falls back to plain text unasked, is held to `require`.
/// Each transaction runs on the one connection, one after the other.
#[derive(Debug)]
pub struct NativePostgres {
    client: Mutex<Client>,
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "it is handed to map_err, which passes each error by value"
)]
fn refused(reason: impl ToString) -> SqlError {
    SqlError::Unavailable {
        reason: reason.to_string(),
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "it is handed to map_err, which passes each error by value"
)]
fn failed(error: tokio_postgres::Error) -> SqlError {
    refused(
        error
            .as_db_error()
            .map_or_else(|| chain(&error), |db| db.message().to_owned()),
    )
}

fn bound(value: &Value) -> Box<dyn ToSql + Sync + Send> {
    match value {
        Value::Null => Box::new(None::<String>),
        Value::Integer(integer) => Box::new(*integer),
        Value::Text(text) => Box::new(text.clone()),
    }
}

fn read(row: &Row, column: usize) -> Result<Value, SqlError> {
    let kind = row
        .columns()
        .get(column)
        .map(|column| column.type_().clone());
    let value = match kind {
        Some(Type::INT8) => row
            .try_get::<_, Option<i64>>(column)
            .map(|value| value.map(Value::Integer)),
        Some(Type::INT4) => row
            .try_get::<_, Option<i32>>(column)
            .map(|value| value.map(|integer| Value::Integer(i64::from(integer)))),
        Some(Type::TEXT | Type::VARCHAR) => row
            .try_get::<_, Option<String>>(column)
            .map(|value| value.map(Value::Text)),
        _ => {
            return Err(refused(
                "a column holds neither an integer nor text".to_owned(),
            ));
        }
    };
    value
        .map(|value| value.unwrap_or(Value::Null))
        .map_err(failed)
}

impl NativePostgres {
    /// The database the connection URL in `url` names, reached over TLS
    /// unless the URL sets `sslmode=disable`.
    ///
    /// # Errors
    ///
    /// When the URL cannot be read, or the database cannot be reached.
    pub async fn connect(url: &Secret) -> Result<Self, SqlError> {
        let mut config: Config = url.expose().parse().map_err(|_| {
            refused(format!(
                "the secret {} is not a Postgres connection URL",
                url.name()
            ))
        })?;
        if config.get_ssl_mode() == SslMode::Prefer {
            config.ssl_mode(SslMode::Require);
        }
        let tls = tls(rustls::crypto::ring::default_provider()).map_err(refused)?;
        let (client, connection) = config
            .connect(MakeRustlsConnect::new(tls))
            .await
            .map_err(failed)?;
        tokio::spawn(connection);
        Ok(Self {
            client: Mutex::new(client),
        })
    }
}

impl Sql for NativePostgres {
    async fn transaction(&self, statements: Vec<Statement>) -> Result<Vec<Changed>, SqlError> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(failed)?;
        let mut answers = Vec::with_capacity(statements.len());
        for statement in &statements {
            let prepared = transaction.prepare(&statement.sql).await.map_err(failed)?;
            let params: Vec<Box<dyn ToSql + Sync + Send>> =
                statement.params.iter().map(bound).collect();
            let borrowed: Vec<&(dyn ToSql + Sync)> = params
                .iter()
                .map(|param| param.as_ref() as &(dyn ToSql + Sync))
                .collect();
            if prepared.columns().is_empty() {
                let changed = transaction
                    .execute(&prepared, &borrowed)
                    .await
                    .map_err(failed)?;
                answers.push(Changed {
                    rows: vec![],
                    changed,
                });
            } else {
                let found = transaction
                    .query(&prepared, &borrowed)
                    .await
                    .map_err(failed)?;
                let rows = found
                    .iter()
                    .map(|row| (0..row.len()).map(|column| read(row, column)).collect())
                    .collect::<Result<_, _>>()?;
                answers.push(Changed { rows, changed: 0 });
            }
        }
        transaction.commit().await.map_err(failed)?;
        Ok(answers)
    }
}
