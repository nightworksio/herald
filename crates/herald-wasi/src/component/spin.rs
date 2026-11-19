//! The databases Spin offers and the models it runs, through its own
//! stable interfaces.

use std::future::{Future, ready};

use herald_ports::{
    Answers, Changed, Infer, InferError, Inference, Secret, Sql, SqlError, Statement, Value,
};
use jiff::SignedDuration;

use herald_spin::llm;
use herald_spin::postgres as pg;
use herald_spin::sqlite;

const UNKEPT: &str = "a column of a type herald keeps no records in";

fn lite(value: Value) -> sqlite::Value {
    match value {
        Value::Null => sqlite::Value::Null,
        Value::Integer(number) => sqlite::Value::Integer(number),
        Value::Text(text) => sqlite::Value::Text(text),
    }
}

fn from_lite(value: sqlite::Value) -> Result<Value, SqlError> {
    match value {
        sqlite::Value::Null => Ok(Value::Null),
        sqlite::Value::Integer(number) => Ok(Value::Integer(number)),
        sqlite::Value::Text(text) => Ok(Value::Text(text)),
        sqlite::Value::Real(_) | sqlite::Value::Blob(_) => Err(SqlError::unavailable(UNKEPT)),
    }
}

fn parameter(value: Value) -> pg::ParameterValue {
    match value {
        Value::Null => pg::ParameterValue::DbNull,
        Value::Integer(number) => pg::ParameterValue::Int64(number),
        Value::Text(text) => pg::ParameterValue::Str(text),
    }
}

fn from_pg(value: pg::DbValue) -> Result<Value, SqlError> {
    Ok(match value {
        pg::DbValue::DbNull => Value::Null,
        pg::DbValue::Boolean(flag) => Value::Integer(i64::from(flag)),
        pg::DbValue::Int8(number) => Value::Integer(i64::from(number)),
        pg::DbValue::Int16(number) => Value::Integer(i64::from(number)),
        pg::DbValue::Int32(number) => Value::Integer(i64::from(number)),
        pg::DbValue::Int64(number) | pg::DbValue::Timestamp(number) => Value::Integer(number),
        pg::DbValue::Str(text) => Value::Text(text),
        _ => return Err(SqlError::unavailable(UNKEPT)),
    })
}

/// The answer to a transaction whose statements answered `done`: committed
/// where they all ran, rolled back where one failed, its error kept and
/// the rollback's added where that failed too.
fn finished(
    done: Result<Vec<Changed>, SqlError>,
    simple: impl Fn(&str) -> Result<(), SqlError>,
) -> Result<Vec<Changed>, SqlError> {
    match done {
        Ok(answers) => simple("COMMIT").map(|()| answers),
        Err(SqlError::Unavailable { reason }) => Err(match simple("ROLLBACK") {
            Ok(()) => SqlError::Unavailable { reason },
            Err(SqlError::Unavailable { reason: rollback }) => SqlError::unavailable(format_args!(
                "{reason}; the rollback failed too: {rollback}"
            )),
        }),
    }
}

fn counted(connection: &sqlite::Connection, sql: &str) -> Result<u64, SqlError> {
    let counted = connection
        .execute(sql, &[])
        .map_err(|error| SqlError::unavailable(format_args!("{error:?}")))?;
    Ok(
        match counted.rows.first().and_then(|row| row.values.first()) {
            Some(sqlite::Value::Integer(count)) => u64::try_from(*count).unwrap_or(0),
            _ => 0,
        },
    )
}

fn sqlite_ran(connection: &sqlite::Connection, statement: Statement) -> Result<Changed, SqlError> {
    let parameters = statement.params.into_iter().map(lite).collect::<Vec<_>>();
    let before = counted(connection, "SELECT total_changes()")?;
    let answered = connection
        .execute(&statement.sql, &parameters)
        .map_err(|error| SqlError::unavailable(format_args!("{error:?}")))?;
    if statement.answers == Answers::Rows {
        return Ok(Changed {
            rows: answered
                .rows
                .into_iter()
                .map(|row| row.values.into_iter().map(from_lite).collect())
                .collect::<Result<_, _>>()?,
            changed: 0,
        });
    }
    let changed = if counted(connection, "SELECT total_changes()")? == before {
        0
    } else {
        counted(connection, "SELECT changes()")?
    };
    Ok(Changed {
        rows: Vec::new(),
        changed,
    })
}

fn postgres_ran(connection: &pg::Connection, statement: Statement) -> Result<Changed, SqlError> {
    let parameters = statement
        .params
        .into_iter()
        .map(parameter)
        .collect::<Vec<_>>();
    if statement.answers == Answers::Rows {
        let answered = connection
            .query(&statement.sql, &parameters)
            .map_err(|error| SqlError::unavailable(format_args!("{error:?}")))?;
        let rows = answered
            .rows
            .into_iter()
            .map(|row| row.into_iter().map(from_pg).collect())
            .collect::<Result<_, _>>()?;
        return Ok(Changed { rows, changed: 0 });
    }
    let changed = connection
        .execute(&statement.sql, &parameters)
        .map_err(|error| SqlError::unavailable(format_args!("{error:?}")))?;
    Ok(Changed {
        rows: Vec::new(),
        changed,
    })
}

/// A database Spin offers. A connection lasts one transaction.
#[derive(Debug)]
pub(super) enum SpinSql {
    Sqlite { label: String, busy: SignedDuration },
    Postgres(Secret),
}

impl SpinSql {
    fn ran(&self, statements: Vec<Statement>) -> Result<Vec<Changed>, SqlError> {
        match self {
            Self::Sqlite { label, busy } => {
                let connection = sqlite::Connection::open(label)
                    .map_err(|error| SqlError::unavailable(format_args!("{error:?}")))?;
                let simple = |sql: &str| {
                    connection
                        .execute(sql, &[])
                        .map(|_| ())
                        .map_err(|error| SqlError::unavailable(format_args!("{error:?}")))
                };
                simple(&format!(
                    "PRAGMA busy_timeout = {}",
                    busy.unsigned_abs().as_millis()
                ))?;
                simple("BEGIN IMMEDIATE")?;
                let done = statements
                    .into_iter()
                    .map(|statement| sqlite_ran(&connection, statement))
                    .collect();
                finished(done, simple)
            }
            Self::Postgres(address) => {
                let connection = pg::Connection::open(address.expose())
                    .map_err(|error| SqlError::unavailable(format_args!("{error:?}")))?;
                let simple = |sql: &str| {
                    connection
                        .execute(sql, &[])
                        .map(|_| ())
                        .map_err(|error| SqlError::unavailable(format_args!("{error:?}")))
                };
                simple("BEGIN")?;
                let done = statements
                    .into_iter()
                    .map(|statement| postgres_ran(&connection, statement))
                    .collect();
                finished(done, simple)
            }
        }
    }
}

impl Sql for SpinSql {
    fn transaction(
        &self,
        statements: Vec<Statement>,
    ) -> impl Future<Output = Result<Vec<Changed>, SqlError>> + Send {
        ready(self.ran(statements))
    }
}

/// What Spin's `llm` interface is asked with but the most tokens: Spin's
/// own defaults.
const PARAMS: llm::InferencingParams = llm::InferencingParams {
    max_tokens: 100,
    repeat_penalty: 1.1,
    repeat_penalty_last_n_token_count: 64,
    temperature: 0.8,
    top_k: 40,
    top_p: 0.9,
};

/// The models Spin runs beside herald, through its `llm` interface
/// (ADR-0059).
#[derive(Debug)]
pub(super) struct SpinLlm;

impl Infer for SpinLlm {
    fn infer(
        &self,
        inference: Inference,
    ) -> impl Future<Output = Result<Vec<u8>, InferError>> + Send {
        let prompt = String::from_utf8_lossy(&inference.inputs);
        let params = llm::InferencingParams {
            max_tokens: inference.tokens,
            ..PARAMS
        };
        ready(match llm::infer(&inference.model, &prompt, Some(params)) {
            Ok(inferred) if inferred.text.len() > inference.most => Err(InferError::TooLarge {
                most: inference.most,
            }),
            Ok(inferred) => Ok(inferred.text.into_bytes()),
            Err(error) => Err(InferError::failed(format!("{error:?}"))),
        })
    }
}
