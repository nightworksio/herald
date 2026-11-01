use std::fmt;
use std::future::Future;

/// A value bound to, or read from, a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// `NULL`.
    Null,
    /// An integer.
    Integer(i64),
    /// Text.
    Text(String),
}

/// One statement and the values bound to its parameters, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// The SQL, with numbered parameters.
    pub sql: String,
    /// The values bound to them.
    pub params: Vec<Value>,
}

/// What one statement of a transaction answered.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Changed {
    /// The rows it returned.
    pub rows: Vec<Vec<Value>>,
    /// How many rows it changed.
    pub changed: u64,
}

/// Why a transaction did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlError {
    /// The database could not be reached or refused the statements.
    Unavailable {
        /// What it said.
        reason: String,
    },
}

impl fmt::Display for SqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { reason } => {
                write!(f, "the database refused the statements: {reason}")
            }
        }
    }
}

/// A database that runs statements in one transaction: SQLite, Postgres or D1.
pub trait Sql: Sync {
    /// Runs `statements` in one transaction, all or none, and answers each.
    fn transaction(
        &self,
        statements: Vec<Statement>,
    ) -> impl Future<Output = Result<Vec<Changed>, SqlError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_reads_as_a_sentence() {
        assert_eq!(
            SqlError::Unavailable {
                reason: "locked".to_owned()
            }
            .to_string(),
            "the database refused the statements: locked"
        );
    }
}
