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

/// What a statement answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answers {
    /// The rows it returns.
    Rows,
    /// How many rows it changed.
    Changes,
}

/// One statement, the values bound to its parameters, in order, and what it
/// answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// The SQL, with numbered parameters.
    pub sql: String,
    /// The values bound to them.
    pub params: Vec<Value>,
    /// What it answers with.
    pub answers: Answers,
}

impl Statement {
    /// A statement that returns rows.
    #[must_use]
    pub fn query(sql: impl Into<String>, params: Vec<Value>) -> Self {
        Self {
            sql: sql.into(),
            params,
            answers: Answers::Rows,
        }
    }

    /// A statement that changes rows and returns none.
    #[must_use]
    pub fn change(sql: impl Into<String>, params: Vec<Value>) -> Self {
        Self {
            sql: sql.into(),
            params,
            answers: Answers::Changes,
        }
    }
}

/// How a database's statements name their parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parameters {
    /// `?1`, `?2`, as SQLite and Cloudflare D1 write them.
    Question,
    /// `$1`, `$2`, as Postgres writes them.
    Dollar,
}

impl Parameters {
    /// How a statement names its parameter at `place`, counted from 1.
    #[must_use]
    pub fn at(self, place: u8) -> String {
        match self {
            Self::Question => format!("?{place}"),
            Self::Dollar => format!("${place}"),
        }
    }
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
    fn a_parameter_is_named_as_its_database_writes_it() {
        assert_eq!(Parameters::Question.at(12), "?12");
        assert_eq!(Parameters::Dollar.at(3), "$3");
    }

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
