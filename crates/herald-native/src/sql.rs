use herald_ports::{Changed, Sql, SqlError, Statement};

use crate::{NativePostgres, NativeSqlite};

/// A database the native binary keeps records in.
#[derive(Debug)]
pub enum NativeSql {
    /// A SQLite file.
    Sqlite(NativeSqlite),
    /// A Postgres database.
    Postgres(NativePostgres),
}

impl Sql for NativeSql {
    async fn transaction(&self, statements: Vec<Statement>) -> Result<Vec<Changed>, SqlError> {
        match self {
            Self::Sqlite(sql) => sql.transaction(statements).await,
            Self::Postgres(sql) => sql.transaction(statements).await,
        }
    }
}
