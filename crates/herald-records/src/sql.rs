use herald_ports::{Changed, Sql, Statement, StoreError, Value};

use crate::records::{Blob, Records, Wrote};

/// The layout of the tables this herald keeps records in.
const LAYOUT: i64 = 1;

/// How a database numbers a statement's parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `?1`, `?2`, as SQLite and Cloudflare D1 write them.
    Sqlite,
    /// `$1`, `$2`, as Postgres writes them.
    Postgres,
}

impl Dialect {
    /// How a statement names its parameter at `place`, counted from 1.
    fn at(self, place: u8) -> String {
        match self {
            Self::Sqlite => format!("?{place}"),
            Self::Postgres => format!("${place}"),
        }
    }

    /// The table records are kept in, its paths ordered byte by byte so a
    /// directory is one range of them.
    const fn records(self) -> &'static str {
        match self {
            Self::Sqlite => {
                "CREATE TABLE IF NOT EXISTS herald_records (path TEXT PRIMARY KEY, body TEXT NOT NULL, version BIGINT NOT NULL)"
            }
            Self::Postgres => {
                "CREATE TABLE IF NOT EXISTS herald_records (path TEXT COLLATE \"C\" PRIMARY KEY, body TEXT NOT NULL, version BIGINT NOT NULL)"
            }
        }
    }
}

/// Records kept one row per path in a SQL database (ADR-0005): each row's
/// version is the database's count of writes when it was written, so a
/// path removed and written again never takes a version back.
#[derive(Debug)]
pub struct SqlRecords<S> {
    sql: S,
    dialect: Dialect,
}

fn unavailable(error: &impl ToString) -> StoreError {
    StoreError::Unavailable {
        reason: error.to_string(),
    }
}

fn corrupt(reason: &str) -> StoreError {
    StoreError::Corrupt {
        reason: reason.to_owned(),
    }
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn changed(answers: &[Changed], index: usize) -> u64 {
    answers.get(index).map_or(0, |answer| answer.changed)
}

fn rows(answers: Vec<Changed>, index: usize) -> Vec<Vec<Value>> {
    answers
        .into_iter()
        .nth(index)
        .map(|answer| answer.rows)
        .unwrap_or_default()
}

fn written(wrote: bool) -> Wrote {
    if wrote {
        Wrote::Written
    } else {
        Wrote::Conflict
    }
}

impl<S: Sql> SqlRecords<S> {
    /// The records `sql` keeps, in `dialect`, with their tables made where
    /// they are missing.
    ///
    /// # Errors
    ///
    /// When the database cannot be reached, or a newer herald laid its
    /// tables out.
    pub async fn open(sql: S, dialect: Dialect) -> Result<Self, StoreError> {
        let records = Self { sql, dialect };
        let answers = records
            .run(vec![
                Statement::change("CREATE TABLE IF NOT EXISTS herald_meta (id INTEGER PRIMARY KEY CHECK (id = 1), layout BIGINT NOT NULL, made BIGINT NOT NULL)", vec![]),
                Statement::change(format!("INSERT INTO herald_meta (id, layout, made) VALUES (1, {}, 0) ON CONFLICT (id) DO NOTHING", dialect.at(1)), vec![Value::Integer(LAYOUT)]),
                Statement::change(dialect.records(), vec![]),
                Statement::query("SELECT layout FROM herald_meta", vec![]),
            ])
            .await?;
        match rows(answers, 3).as_slice() {
            [row] if row.as_slice() == [Value::Integer(LAYOUT)] => Ok(records),
            [row] if matches!(row.as_slice(), [Value::Integer(layout)] if *layout > LAYOUT) => Err(
                corrupt("a newer herald laid the database out; run that herald or a later one"),
            ),
            _ => Err(corrupt("the database's herald_meta table is not herald's")),
        }
    }

    fn at(&self, place: u8) -> String {
        self.dialect.at(place)
    }

    async fn run(&self, statements: Vec<Statement>) -> Result<Vec<Changed>, StoreError> {
        self.sql
            .transaction(statements)
            .await
            .map_err(|error| unavailable(&error))
    }
}

impl<S: Sql> Records for SqlRecords<S> {
    async fn read(&self, path: &str) -> Result<Option<Blob>, StoreError> {
        let answers = self
            .run(vec![Statement::query(
                format!(
                    "SELECT body, version FROM herald_records WHERE path = {}",
                    self.at(1)
                ),
                vec![text(path)],
            )])
            .await?;
        match rows(answers, 0).as_slice() {
            [] => Ok(None),
            [row] => match row.as_slice() {
                [Value::Text(body), Value::Integer(version)] => Ok(Some(Blob {
                    token: version.to_string(),
                    bytes: body.clone().into_bytes(),
                })),
                _ => Err(corrupt("a record's row is not a body and a version")),
            },
            _ => Err(corrupt("a path holds more than one record")),
        }
    }

    async fn list(&self, directory: &str) -> Result<Vec<String>, StoreError> {
        let answers = self
            .run(vec![Statement::query(
                format!(
                    "SELECT path FROM herald_records WHERE path >= {} AND path < {} ORDER BY path",
                    self.at(1),
                    self.at(2)
                ),
                vec![
                    text(&format!("{directory}/")),
                    text(&format!("{directory}0")),
                ],
            )])
            .await?;
        rows(answers, 0)
            .into_iter()
            .map(|row| match row.as_slice() {
                [Value::Text(path)] => Ok(path.clone()),
                _ => Err(corrupt("a record's path is not text")),
            })
            .collect()
    }

    async fn write(
        &self,
        path: &str,
        bytes: &[u8],
        replacing: Option<&str>,
    ) -> Result<Wrote, StoreError> {
        let body = std::str::from_utf8(bytes).map_err(|_| corrupt("a record is not UTF-8 text"))?;
        let counted = Statement::change("UPDATE herald_meta SET made = made + 1", vec![]);
        let answers = match replacing {
            None => {
                self.run(vec![
                    counted,
                    Statement::change(format!("INSERT INTO herald_records (path, body, version) SELECT {}, {}, made FROM herald_meta WHERE true ON CONFLICT (path) DO NOTHING", self.at(1), self.at(2)), vec![text(path), text(body)]),
                ])
                .await?
            }
            Some(token) => {
                let Ok(version) = token.parse::<i64>() else {
                    return Ok(Wrote::Conflict);
                };
                self.run(vec![
                    counted,
                    Statement::change(format!("UPDATE herald_records SET body = {}, version = (SELECT made FROM herald_meta) WHERE path = {} AND version = {}", self.at(2), self.at(1), self.at(3)), vec![text(path), text(body), Value::Integer(version)]),
                ])
                .await?
            }
        };
        Ok(written(changed(&answers, 1) == 1))
    }

    async fn remove(&self, path: &str, replacing: &str) -> Result<Wrote, StoreError> {
        let Ok(version) = replacing.parse::<i64>() else {
            return Ok(Wrote::Conflict);
        };
        let answers = self
            .run(vec![
                Statement::change(
                    format!(
                        "DELETE FROM herald_records WHERE path = {} AND version = {}",
                        self.at(1),
                        self.at(2)
                    ),
                    vec![text(path), Value::Integer(version)],
                ),
                Statement::query(
                    format!(
                        "SELECT version FROM herald_records WHERE path = {}",
                        self.at(1)
                    ),
                    vec![text(path)],
                ),
            ])
            .await?;
        Ok(written(
            changed(&answers, 0) == 1 || rows(answers, 1).is_empty(),
        ))
    }
}
