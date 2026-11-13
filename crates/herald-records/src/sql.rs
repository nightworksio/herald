use herald_ports::{Changed, Parameters, Sql, Statement, StoreError, Value};

use crate::records::{Blob, Records, Wrote};

/// The layout of the tables this herald keeps records in.
pub const LAYOUT: i64 = 2;

/// The SQL a database speaks: SQLite's, as Cloudflare D1 speaks it too, or
/// Postgres's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// SQLite's.
    Sqlite,
    /// Postgres's.
    Postgres,
}

impl Dialect {
    /// How its statements name their parameters.
    #[must_use]
    pub const fn parameters(self) -> Parameters {
        match self {
            Self::Sqlite => Parameters::Question,
            Self::Postgres => Parameters::Dollar,
        }
    }

    /// How a statement names its parameter at `place`, counted from 1.
    fn at(self, place: u8) -> String {
        self.parameters().at(place)
    }

    /// The table records are kept in, its paths ordered byte by byte so a
    /// directory is one range of them.
    const fn records(self) -> &'static str {
        match self {
            Self::Sqlite => {
                "CREATE TABLE IF NOT EXISTS herald_records (path TEXT PRIMARY KEY, body TEXT NOT NULL, epoch TEXT NOT NULL, version BIGINT NOT NULL)"
            }
            Self::Postgres => {
                "CREATE TABLE IF NOT EXISTS herald_records (path TEXT COLLATE \"C\" PRIMARY KEY, body TEXT NOT NULL, epoch TEXT NOT NULL, version BIGINT NOT NULL)"
            }
        }
    }

    /// An expression the database evaluates to a new random epoch, 32 hex
    /// digits.
    const fn epoch(self) -> &'static str {
        match self {
            Self::Sqlite => "lower(hex(randomblob(16)))",
            Self::Postgres => "replace(gen_random_uuid()::text, '-', '')",
        }
    }
}

/// A record's token: the epoch it drew when it was made, and how many
/// times it has been written since.
fn token(epoch: &str, version: i64) -> String {
    format!("{epoch}.{version}")
}

/// The epoch and the version a token names, where it names them.
fn parsed(token: &str) -> Option<(String, i64)> {
    let (epoch, version) = token.split_once('.')?;
    Some((epoch.to_owned(), version.parse().ok()?))
}

/// Records kept one row per path in a SQL database (ADR-0005). Each row
/// counts its own writes, under a random epoch it draws when it is made, so
/// a path removed and written again never takes an earlier token back.
#[derive(Debug)]
pub struct SqlRecords<S> {
    sql: S,
    dialect: Dialect,
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
                Statement::change("CREATE TABLE IF NOT EXISTS herald_meta (id INTEGER PRIMARY KEY CHECK (id = 1), layout BIGINT NOT NULL)", vec![]),
                Statement::change(format!("INSERT INTO herald_meta (id, layout) SELECT 1, {} WHERE NOT EXISTS (SELECT 1 FROM herald_meta) ON CONFLICT (id) DO NOTHING", dialect.at(1)), vec![Value::Integer(LAYOUT)]),
                Statement::change(dialect.records(), vec![]),
                Statement::query("SELECT layout FROM herald_meta", vec![]),
            ])
            .await?;
        match rows(answers, 3).as_slice() {
            [row] if row.as_slice() == [Value::Integer(LAYOUT)] => Ok(records),
            [row] if matches!(row.as_slice(), [Value::Integer(layout)] if *layout > LAYOUT) => {
                Err(StoreError::corrupt(
                    "a newer herald laid the database out; run that herald or a later one",
                ))
            }
            [row] if matches!(row.as_slice(), [Value::Integer(layout)] if *layout > 0) => {
                Err(StoreError::corrupt(
                    "a preview of herald laid the database out; give this herald an empty database",
                ))
            }
            _ => Err(StoreError::corrupt(
                "the database's herald_meta table is not herald's",
            )),
        }
    }

    fn at(&self, place: u8) -> String {
        self.dialect.at(place)
    }

    async fn run(&self, statements: Vec<Statement>) -> Result<Vec<Changed>, StoreError> {
        self.sql
            .transaction(statements)
            .await
            .map_err(StoreError::unavailable)
    }
}

impl<S: Sql> Records for SqlRecords<S> {
    async fn read(&self, path: &str) -> Result<Option<Blob>, StoreError> {
        let answers = self
            .run(vec![Statement::query(
                format!(
                    "SELECT body, epoch, version FROM herald_records WHERE path = {}",
                    self.at(1)
                ),
                vec![text(path)],
            )])
            .await?;
        match rows(answers, 0).as_slice() {
            [] => Ok(None),
            [row] => match row.as_slice() {
                [
                    Value::Text(body),
                    Value::Text(epoch),
                    Value::Integer(version),
                ] => Ok(Some(Blob {
                    token: token(epoch, *version),
                    bytes: body.clone().into_bytes(),
                })),
                _ => Err(StoreError::corrupt(
                    "a record's row is not a body, an epoch and a version",
                )),
            },
            _ => Err(StoreError::corrupt("a path holds more than one record")),
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
                _ => Err(StoreError::corrupt("a record's path is not text")),
            })
            .collect()
    }

    async fn write(
        &self,
        path: &str,
        bytes: &[u8],
        replacing: Option<&str>,
    ) -> Result<Wrote, StoreError> {
        let body = std::str::from_utf8(bytes)
            .map_err(|_| StoreError::corrupt("a record is not UTF-8 text"))?;
        let statement = match replacing {
            None => Statement::change(
                format!(
                    "INSERT INTO herald_records (path, body, epoch, version) VALUES ({}, {}, {}, 1) ON CONFLICT (path) DO NOTHING",
                    self.at(1),
                    self.at(2),
                    self.dialect.epoch()
                ),
                vec![text(path), text(body)],
            ),
            Some(replaced) => {
                let Some((epoch, version)) = parsed(replaced) else {
                    return Ok(Wrote::Conflict);
                };
                Statement::change(
                    format!(
                        "UPDATE herald_records SET body = {}, version = version + 1 WHERE path = {} AND epoch = {} AND version = {}",
                        self.at(2),
                        self.at(1),
                        self.at(3),
                        self.at(4)
                    ),
                    vec![
                        text(path),
                        text(body),
                        text(&epoch),
                        Value::Integer(version),
                    ],
                )
            }
        };
        let answers = self.run(vec![statement]).await?;
        Ok(written(changed(&answers, 0) == 1))
    }

    async fn remove(&self, path: &str, replacing: &str) -> Result<Wrote, StoreError> {
        let Some((epoch, version)) = parsed(replacing) else {
            return Ok(Wrote::Conflict);
        };
        let answers = self
            .run(vec![
                Statement::change(
                    format!(
                        "DELETE FROM herald_records WHERE path = {} AND epoch = {} AND version = {}",
                        self.at(1),
                        self.at(2),
                        self.at(3)
                    ),
                    vec![text(path), text(&epoch), Value::Integer(version)],
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
