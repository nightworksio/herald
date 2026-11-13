use std::sync::{Arc, PoisonError};

use herald_ports::{Changed, Secret, Sql, SqlError, Statement, Value};
use jiff::SignedDuration;
use tokio::sync::Mutex;
use tokio_postgres::config::SslMode;
use tokio_postgres::types::{ToSql, Type};
use tokio_postgres::{Client, Config, Row};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::http::{chain, tls};

/// A Postgres database, through which the `postgres` store keeps its records
/// (ADR-0005). The connection is TLS unless its URL sets `sslmode=disable`:
/// `prefer`, which falls back to plain text unasked, is held to `require`.
/// Each transaction runs on the one connection, one after the other, and a
/// connection the database closed is made again before the next one.
pub struct NativePostgres {
    config: Config,
    tls: MakeRustlsConnect,
    held: Mutex<Held>,
}

impl std::fmt::Debug for NativePostgres {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativePostgres")
            .finish_non_exhaustive()
    }
}

/// The connection, and why it closed once it has.
struct Held {
    client: Client,
    closed: Arc<std::sync::Mutex<Option<String>>>,
}

impl Held {
    async fn made(config: &Config, tls: &MakeRustlsConnect) -> Result<Self, SqlError> {
        let (client, connection) = config.connect(tls.clone()).await.map_err(failed)?;
        let closed = Arc::new(std::sync::Mutex::new(None));
        let said = Arc::clone(&closed);
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                *said.lock().unwrap_or_else(PoisonError::into_inner) = Some(chain(&error));
            }
        });
        Ok(Self { client, closed })
    }

    fn why_closed(&self) -> String {
        self.closed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or_else(|| "the database closed it".to_owned())
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "it is handed to map_err, which passes each error by value"
)]
fn failed(error: tokio_postgres::Error) -> SqlError {
    SqlError::unavailable(
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
            return Err(SqlError::unavailable(
                "a column holds neither an integer nor text".to_owned(),
            ));
        }
    };
    value
        .map(|value| value.unwrap_or(Value::Null))
        .map_err(failed)
}

/// How long making a connection and running one statement may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// Making a connection.
    pub connect: SignedDuration,
    /// One statement, after which the database cancels it.
    pub statement: SignedDuration,
}

impl NativePostgres {
    /// The database the connection URL in `url` names, reached over TLS
    /// unless the URL sets `sslmode=disable`, within `timeouts`.
    ///
    /// # Errors
    ///
    /// When the URL cannot be read, or the database cannot be reached.
    pub async fn connect(url: &Secret, timeouts: Timeouts) -> Result<Self, SqlError> {
        let mut config: Config = url.expose().parse().map_err(|_| {
            SqlError::unavailable(format_args!(
                "the secret {} is not a Postgres connection URL",
                url.name()
            ))
        })?;
        if config.get_ssl_mode() == SslMode::Prefer {
            config.ssl_mode(SslMode::Require);
        }
        config.connect_timeout(timeouts.connect.unsigned_abs());
        let limit = format!(
            "-c statement_timeout={}",
            timeouts.statement.unsigned_abs().as_millis()
        );
        let options = match config.get_options() {
            Some(options) => format!("{options} {limit}"),
            None => limit,
        };
        config.options(options);
        let tls = MakeRustlsConnect::new(
            tls(rustls::crypto::ring::default_provider()).map_err(SqlError::unavailable)?,
        );
        let held = Mutex::new(Box::pin(Held::made(&config, &tls)).await?);
        Ok(Self { config, tls, held })
    }
}

impl Sql for NativePostgres {
    async fn transaction(&self, statements: Vec<Statement>) -> Result<Vec<Changed>, SqlError> {
        let mut held = self.held.lock().await;
        if held.client.is_closed() {
            let why = held.why_closed();
            *held = Box::pin(Held::made(&self.config, &self.tls))
                .await
                .map_err(|SqlError::Unavailable { reason }| {
                    SqlError::unavailable(format_args!(
                        "the connection closed ({why}) and could not be made again: {reason}"
                    ))
                })?;
        }
        let transaction = held.client.transaction().await.map_err(failed)?;
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
