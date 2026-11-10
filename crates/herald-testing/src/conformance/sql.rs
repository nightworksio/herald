use herald_ports::{Changed, Parameters, Sql, SqlError, Statement, Value};

use super::{Checks, Failure};

impl From<SqlError> for Failure {
    fn from(error: SqlError) -> Self {
        Self(format!("the database failed: {error}"))
    }
}

/// The largest integer every host carries exactly, JavaScript's included:
/// 2^53 - 1.
const LARGEST: i64 = 9_007_199_254_740_991;

/// The table the suite makes, and drops again.
const TABLE: &str = "herald_conformance";

/// Text past ASCII, so a database that keeps bytes as another encoding
/// fails.
const TEXT: &str = "héraldé ✓";

const fn changed(changed: u64) -> Changed {
    Changed {
        rows: Vec::new(),
        changed,
    }
}

fn rows(rows: Vec<Vec<Value>>) -> Changed {
    Changed { rows, changed: 0 }
}

/// Whether `sql`, whose statements name their parameters as `parameters`
/// says, runs each transaction all or none, binds and reads back every kind
/// of value, and answers how many rows each statement changed and nothing
/// for one that changed none. Every database passes this suite over a
/// table named `herald_conformance` it may drop, and so does
/// [`crate::ScriptedSql`] scripted with the answers such a database gives.
///
/// # Errors
///
/// The first way the database fails, as a sentence.
pub async fn sql_conformance<S: Sql>(sql: &S, parameters: Parameters) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let [made, failed, counted, changing, dropped] = transactions(parameters);
    let [making, counting, changes] = expected();
    let answers = sql.transaction(made).await?;
    checks.ensure(
        answers.get(..4) == making.get(..4),
        "a statement did not answer how many rows it changed, or answered for one that changed none",
    );
    checks.ensure(
        answers.get(4) == making.get(4),
        "a row was not read back with every value as it was bound",
    );
    checks.ensure(
        answers.len() == making.len(),
        "a transaction did not answer each of its statements",
    );
    checks.ensure(
        sql.transaction(failed).await.is_err(),
        "a transaction holding a statement the database refuses was not refused",
    );
    checks.ensure(
        sql.transaction(counted).await? == counting,
        "a transaction that was refused kept a statement it ran before the refusal",
    );
    checks.ensure(
        sql.transaction(changing).await? == changes,
        "an update or delete did not answer how many rows it changed",
    );
    sql.transaction(dropped).await?;
    checks.done()
}

fn transactions(parameters: Parameters) -> [Vec<Statement>; 5] {
    let at = |place| parameters.at(place);
    [
        vec![
            Statement::change(format!("DROP TABLE IF EXISTS {TABLE}"), vec![]),
            Statement::change(
                format!("CREATE TABLE {TABLE} (k BIGINT PRIMARY KEY, n BIGINT, t TEXT)"),
                vec![],
            ),
            Statement::change(
                format!(
                    "INSERT INTO {TABLE} (k, n, t) VALUES (1, {}, {}), (2, {}, {})",
                    at(1),
                    at(2),
                    at(3),
                    at(4)
                ),
                vec![
                    Value::Integer(LARGEST),
                    Value::Text(TEXT.to_owned()),
                    Value::Integer(-LARGEST),
                    Value::Null,
                ],
            ),
            Statement::change(format!("CREATE INDEX {TABLE}_n ON {TABLE} (n)"), vec![]),
            Statement::query(format!("SELECT k, n, t FROM {TABLE} ORDER BY k"), vec![]),
        ],
        vec![
            Statement::change(
                format!("INSERT INTO {TABLE} (k, n, t) VALUES (3, 3, 'kept?')"),
                vec![],
            ),
            Statement::change(
                format!("INSERT INTO {TABLE} (k, n, t) VALUES (1, 1, 'twice')"),
                vec![],
            ),
        ],
        vec![Statement::query(
            format!("SELECT count(*) FROM {TABLE}"),
            vec![],
        )],
        vec![
            Statement::change(
                format!("UPDATE {TABLE} SET t = {} WHERE n < {}", at(1), at(2)),
                vec![Value::Text("updated".to_owned()), Value::Integer(0)],
            ),
            Statement::change(format!("UPDATE {TABLE} SET t = 'none' WHERE k > 9"), vec![]),
            Statement::change(
                format!("DELETE FROM {TABLE} WHERE k = {}", at(1)),
                vec![Value::Integer(1)],
            ),
            Statement::query(
                format!("SELECT k, t FROM {TABLE} WHERE k > {}", at(1)),
                vec![Value::Integer(0)],
            ),
            Statement::query(format!("SELECT k FROM {TABLE} WHERE k > 9"), vec![]),
        ],
        vec![Statement::change(format!("DROP TABLE {TABLE}"), vec![])],
    ]
}

/// What a database passing the suite answers the transactions that make,
/// count and change its rows with.
fn expected() -> [Vec<Changed>; 3] {
    [
        vec![
            changed(0),
            changed(0),
            changed(2),
            changed(0),
            rows(vec![
                vec![
                    Value::Integer(1),
                    Value::Integer(LARGEST),
                    Value::Text(TEXT.to_owned()),
                ],
                vec![Value::Integer(2), Value::Integer(-LARGEST), Value::Null],
            ]),
        ],
        vec![rows(vec![vec![Value::Integer(2)]])],
        vec![
            changed(1),
            changed(0),
            changed(1),
            rows(vec![vec![
                Value::Integer(2),
                Value::Text("updated".to_owned()),
            ]]),
            rows(Vec::new()),
        ],
    ]
}

#[cfg(test)]
mod tests;
