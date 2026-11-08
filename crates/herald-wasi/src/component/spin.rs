//! The databases Spin offers, through its own stable interfaces.

use std::future::{Future, ready};

use herald_ports::{Answers, Changed, Secret, Sql, SqlError, Statement, Value};
use jiff::SignedDuration;

use herald_spin::postgres as pg;
use herald_spin::sqlite;

const UNKEPT: &str = "a column of a type herald keeps no records in";

fn failed(reason: &impl ToString) -> SqlError {
    SqlError::Unavailable {
        reason: reason.to_string(),
    }
}

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
        sqlite::Value::Real(_) | sqlite::Value::Blob(_) => Err(failed(&UNKEPT)),
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
        _ => return Err(failed(&UNKEPT)),
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
            Err(SqlError::Unavailable { reason: rollback }) => SqlError::Unavailable {
                reason: format!("{reason}; the rollback failed too: {rollback}"),
            },
        }),
    }
}

fn sqlite_ran(connection: &sqlite::Connection, statement: Statement) -> Result<Changed, SqlError> {
    let parameters = statement.params.into_iter().map(lite).collect::<Vec<_>>();
    let answered = connection
        .execute(&statement.sql, &parameters)
        .map_err(|error| failed(&format!("{error:?}")))?;
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
    let counted = connection
        .execute("SELECT changes()", &[])
        .map_err(|error| failed(&format!("{error:?}")))?;
    let changed = match counted.rows.first().and_then(|row| row.values.first()) {
        Some(sqlite::Value::Integer(count)) => u64::try_from(*count).unwrap_or(0),
        _ => 0,
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
            .map_err(|error| failed(&format!("{error:?}")))?;
        let rows = answered
            .rows
            .into_iter()
            .map(|row| row.into_iter().map(from_pg).collect())
            .collect::<Result<_, _>>()?;
        return Ok(Changed { rows, changed: 0 });
    }
    let changed = connection
        .execute(&statement.sql, &parameters)
        .map_err(|error| failed(&format!("{error:?}")))?;
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
                    .map_err(|error| failed(&format!("{error:?}")))?;
                let simple = |sql: &str| {
                    connection
                        .execute(sql, &[])
                        .map(|_| ())
                        .map_err(|error| failed(&format!("{error:?}")))
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
                    .map_err(|error| failed(&format!("{error:?}")))?;
                let simple = |sql: &str| {
                    connection
                        .execute(sql, &[])
                        .map(|_| ())
                        .map_err(|error| failed(&format!("{error:?}")))
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
