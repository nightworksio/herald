use std::collections::VecDeque;
use std::sync::Mutex;

use herald_ports::{Changed, Sql, SqlError, Statement};

use crate::lock;

/// A database that answers each transaction with the next scripted answer,
/// and keeps every transaction it was sent.
#[derive(Debug, Default)]
pub struct ScriptedSql {
    answers: Mutex<VecDeque<Result<Vec<Changed>, SqlError>>>,
    sent: Mutex<Vec<Vec<Statement>>>,
}

impl ScriptedSql {
    /// The database with `answer` queued for the next transaction.
    #[must_use]
    pub fn answering(self, answer: Result<Vec<Changed>, SqlError>) -> Self {
        lock(&self.answers).push_back(answer);
        self
    }

    /// Every transaction sent, in order.
    #[must_use]
    pub fn sent(&self) -> Vec<Vec<Statement>> {
        lock(&self.sent).clone()
    }
}

impl Sql for ScriptedSql {
    async fn transaction(&self, statements: Vec<Statement>) -> Result<Vec<Changed>, SqlError> {
        lock(&self.sent).push(statements);
        lock(&self.answers).pop_front().unwrap_or_else(|| {
            Err(SqlError::Unavailable {
                reason: "no answer was scripted".to_owned(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use herald_ports::Value;

    use super::*;

    #[test]
    fn each_transaction_gets_the_next_answer_and_is_kept() {
        let sql = ScriptedSql::default().answering(Ok(vec![Changed {
            rows: vec![vec![Value::Integer(1)]],
            changed: 0,
        }]));
        let statement = Statement {
            sql: "SELECT 1".to_owned(),
            params: vec![],
        };
        let answer = pollster::block_on(sql.transaction(vec![statement.clone()])).unwrap();
        assert_eq!(answer[0].rows, [[Value::Integer(1)]]);
        assert!(pollster::block_on(sql.transaction(vec![])).is_err());
        assert_eq!(sql.sent(), [vec![statement], vec![]]);
    }
}
