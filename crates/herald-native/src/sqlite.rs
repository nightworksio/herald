use std::fs::OpenOptions;
use std::future::Future;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use herald_ports::{Changed, Sql, SqlError, Statement, Value};
use jiff::SignedDuration;
use rusqlite::types::{ToSqlOutput, ValueRef};
use rusqlite::{Connection, TransactionBehavior, params_from_iter};

/// A SQLite database in one file, or in memory, through which the `sqlite`
/// store keeps its records (ADR-0005). Each transaction takes the write
/// lock as it begins, so two processes on one file run theirs one after
/// the other, and runs on a blocking thread, so waiting for the lock holds
/// up no other work.
#[derive(Debug)]
pub struct NativeSqlite {
    connection: Arc<Mutex<Connection>>,
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "it is handed to map_err, which passes each error by value"
)]
fn refused(error: impl ToString) -> SqlError {
    SqlError::Unavailable {
        reason: error.to_string(),
    }
}

/// Makes the file at `path`, readable and writable by its owner alone,
/// where there is none.
fn made(path: &Path) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    match options.open(path) {
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
        opened => opened.map(drop),
    }
}

fn bound(value: &Value) -> ToSqlOutput<'_> {
    ToSqlOutput::Borrowed(match value {
        Value::Null => ValueRef::Null,
        Value::Integer(integer) => ValueRef::Integer(*integer),
        Value::Text(text) => ValueRef::Text(text.as_bytes()),
    })
}

fn read(value: ValueRef<'_>) -> Result<Value, SqlError> {
    match value {
        ValueRef::Null => Ok(Value::Null),
        ValueRef::Integer(integer) => Ok(Value::Integer(integer)),
        ValueRef::Text(text) => std::str::from_utf8(text)
            .map(|text| Value::Text(text.to_owned()))
            .map_err(refused),
        ValueRef::Real(_) | ValueRef::Blob(_) => {
            Err(refused("a column holds neither an integer nor text"))
        }
    }
}

fn transact(
    connection: &mut Connection,
    statements: &[Statement],
) -> Result<Vec<Changed>, SqlError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(refused)?;
    let mut answers = Vec::with_capacity(statements.len());
    for statement in statements {
        let before = transaction.total_changes();
        let mut prepared = transaction.prepare(&statement.sql).map_err(refused)?;
        let columns = prepared.column_count();
        let mut query = prepared
            .query(params_from_iter(statement.params.iter().map(bound)))
            .map_err(refused)?;
        let mut rows = Vec::new();
        while let Some(row) = query.next().map_err(refused)? {
            rows.push(
                (0..columns)
                    .map(|column| read(row.get_ref(column).map_err(refused)?))
                    .collect::<Result<_, _>>()?,
            );
        }
        drop(query);
        drop(prepared);
        answers.push(Changed {
            rows,
            changed: transaction.total_changes() - before,
        });
    }
    transaction.commit().map_err(refused)?;
    Ok(answers)
}

impl NativeSqlite {
    /// The database in the file at `path`, made readable by its owner alone
    /// where it is new, whose transactions wait up to `busy` for another
    /// process to let go of it.
    ///
    /// # Errors
    ///
    /// When the file cannot be made or opened as a SQLite database.
    pub fn open(path: &Path, busy: SignedDuration) -> Result<Self, SqlError> {
        made(path)
            .map_err(|error| refused(format!("{} could not be made: {error}", path.display())))?;
        let connection = Connection::open(path).map_err(refused)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(refused)?;
        connection
            .busy_timeout(busy.unsigned_abs())
            .map_err(refused)?;
        Ok(Self::kept(connection))
    }

    /// A database in memory, gone when it is dropped.
    ///
    /// # Errors
    ///
    /// When SQLite cannot open one.
    pub fn in_memory() -> Result<Self, SqlError> {
        Ok(Self::kept(Connection::open_in_memory().map_err(refused)?))
    }

    fn kept(connection: Connection) -> Self {
        Self {
            connection: Arc::new(Mutex::new(connection)),
        }
    }
}

impl Sql for NativeSqlite {
    fn transaction(
        &self,
        statements: Vec<Statement>,
    ) -> impl Future<Output = Result<Vec<Changed>, SqlError>> + Send {
        let connection = Arc::clone(&self.connection);
        async move {
            tokio::task::spawn_blocking(move || {
                let mut connection = connection.lock().unwrap_or_else(PoisonError::into_inner);
                transact(&mut connection, &statements)
            })
            .await
            .map_err(refused)?
        }
    }
}
