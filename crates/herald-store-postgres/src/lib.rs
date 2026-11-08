//! herald's records kept one row per record in a Postgres database
//! (ADR-0005), for the CLI, the Lambda and the container.
//!
//! Each write names the version it replaces, and a transaction changes the
//! row only where it still has that version, which a concurrent writer's
//! row lock makes it wait to see: that is the compare-and-set.

use herald_core::{Duration, SecretRef};
use herald_ports::{Sql, StoreError};
use herald_records::{Dialect, Kept, SqlRecords};
use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::Deserialize;

/// How long making a connection may take where the options name no time.
pub const CONNECT_TIMEOUT: SignedDuration = SignedDuration::from_secs(10);

/// How long one statement may run where the options name no time.
pub const STATEMENT_TIMEOUT: SignedDuration = SignedDuration::from_secs(30);

/// The postgres store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The secret holding the database's connection URL, such as
    /// `postgres://herald:…@db.example/herald`. herald reaches it over TLS
    /// unless the URL sets `sslmode=disable`.
    pub url: SecretRef,
    /// How long making a connection may take; 10s where absent.
    pub connect_timeout: Option<Duration>,
    /// How long one statement may run before the database cancels it; 30s
    /// where absent.
    pub statement_timeout: Option<Duration>,
    /// The secret holding a key to seal every record with, such as
    /// `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt
    /// (ADR-0005); records are not sealed where absent.
    pub key: Option<SecretRef>,
}

impl StoreOptions {
    /// How long making a connection may take.
    #[must_use]
    pub fn connect_timeout(&self) -> SignedDuration {
        self.connect_timeout
            .map_or(CONNECT_TIMEOUT, |Duration(timeout)| timeout)
    }

    /// How long one statement may run before the database cancels it.
    #[must_use]
    pub fn statement_timeout(&self) -> SignedDuration {
        self.statement_timeout
            .map_or(STATEMENT_TIMEOUT, |Duration(timeout)| timeout)
    }
}

/// The connection address `address`, a URL or `key=value` pairs, with
/// `connect` and `statement` as its connection and statement timeouts, for
/// a host that hands the address to its database driver as it is. A timeout
/// the address already sets is kept as it is written.
#[must_use]
pub fn within(address: &str, connect: SignedDuration, statement: SignedDuration) -> String {
    let keyed = !address.contains("://");
    let mut added = Vec::new();
    if !address.contains("connect_timeout=") {
        let seconds = connect.unsigned_abs().as_secs().max(1);
        added.push(format!("connect_timeout={seconds}"));
    }
    if !address.contains("options=") {
        let millis = statement.unsigned_abs().as_millis();
        added.push(if keyed {
            format!("options='-c statement_timeout={millis}'")
        } else {
            format!("options=-c%20statement_timeout%3D{millis}")
        });
    }
    if added.is_empty() {
        address.to_owned()
    } else if keyed {
        format!("{address} {}", added.join(" "))
    } else {
        let separator = if address.contains('?') { '&' } else { '?' };
        format!("{address}{separator}{}", added.join("&"))
    }
}

/// The store a Postgres database keeps.
pub type PostgresStore<S> = Kept<SqlRecords<S>>;

/// The store kept in `sql`, a Postgres database, with its tables made where
/// they are missing.
///
/// # Errors
///
/// When the database cannot be reached, or a newer herald laid it out.
pub async fn store<S: Sql>(sql: S) -> Result<PostgresStore<S>, StoreError> {
    SqlRecords::open(sql, Dialect::Postgres)
        .await
        .map(Kept::new)
}

#[cfg(test)]
mod tests {
    use herald_ports::{Changed, SqlError, Store as _, Value};
    use herald_testing::ScriptedSql;

    use super::*;

    #[test]
    fn an_address_takes_the_timeouts_it_does_not_set_itself() {
        let ten = SignedDuration::from_secs(10);
        let half = SignedDuration::from_millis(500);
        assert_eq!(
            within("postgres://db/herald", ten, half),
            "postgres://db/herald?connect_timeout=10&options=-c%20statement_timeout%3D500"
        );
        assert_eq!(
            within("postgres://db/herald?sslmode=require", half, ten),
            "postgres://db/herald?sslmode=require&connect_timeout=1&options=-c%20statement_timeout%3D10000"
        );
        assert_eq!(
            within("host=db dbname=herald", ten, half),
            "host=db dbname=herald connect_timeout=10 options='-c statement_timeout=500'"
        );
        assert_eq!(
            within("postgres://db/herald?connect_timeout=3", ten, half),
            "postgres://db/herald?connect_timeout=3&options=-c%20statement_timeout%3D500"
        );
        let both = "host=db connect_timeout=3 options='-c search_path=herald'";
        assert_eq!(within(both, ten, half), both);
    }

    #[test]
    fn the_timeouts_are_the_ones_named_or_else_ten_and_thirty_seconds() {
        let url = SecretRef {
            secret: "HERALD_POSTGRES_URL".to_owned(),
        };
        let unnamed = StoreOptions {
            key: None,
            url: url.clone(),
            connect_timeout: None,
            statement_timeout: None,
        };
        assert_eq!(unnamed.connect_timeout(), CONNECT_TIMEOUT);
        assert_eq!(unnamed.statement_timeout(), STATEMENT_TIMEOUT);
        let named = StoreOptions {
            key: None,
            url,
            connect_timeout: Some(Duration(SignedDuration::from_secs(3))),
            statement_timeout: Some(Duration(SignedDuration::from_secs(4))),
        };
        assert_eq!(named.connect_timeout(), SignedDuration::from_secs(3));
        assert_eq!(named.statement_timeout(), SignedDuration::from_secs(4));
    }

    #[test]
    fn a_database_that_cannot_be_reached_opens_no_store() {
        let unreachable = ScriptedSql::default().answering(Err(SqlError::Unavailable {
            reason: "refused".to_owned(),
        }));
        assert_eq!(
            pollster::block_on(store(unreachable)).err(),
            Some(StoreError::Unavailable {
                reason: "the database refused the statements: refused".to_owned()
            })
        );
        let opened = ScriptedSql::default().answering(Ok(vec![
            Changed::default(),
            Changed::default(),
            Changed::default(),
            Changed {
                rows: vec![vec![Value::Integer(herald_records::LAYOUT)]],
                changed: 0,
            },
        ]));
        assert!(
            pollster::block_on(store(opened))
                .unwrap()
                .cursors()
                .is_some()
        );
    }
}
