use herald_core::SecretRef;
use schemars::JsonSchema;
use serde::Deserialize;

/// The table the records are kept in where the options name none.
pub const TABLE: &str = "herald";
/// The secret holding the access key's id where the options name none.
pub const ACCESS_KEY_ID: &str = "AWS_ACCESS_KEY_ID";
/// The secret holding the secret access key where the options name none.
pub const SECRET_ACCESS_KEY: &str = "AWS_SECRET_ACCESS_KEY";
/// The secret holding a session token where the options name none.
pub const SESSION_TOKEN: &str = "AWS_SESSION_TOKEN";
/// Where the region is read from where the options name none.
pub const REGION: &str = "AWS_REGION";

/// The dynamodb store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The table, with the string partition key `directory` and the string
    /// sort key `name`; `herald` where absent.
    pub table: Option<String>,
    /// The region, such as `eu-west-1`; the `AWS_REGION` the platform sets
    /// where absent.
    pub region: Option<String>,
    /// The endpoint's base URL, for DynamoDB Local or another endpoint;
    /// the region's own where absent.
    pub endpoint: Option<String>,
    /// The secret holding the access key's id; `AWS_ACCESS_KEY_ID` where
    /// absent, as a Lambda's role provides it.
    pub access_key_id: Option<SecretRef>,
    /// The secret holding the secret access key; `AWS_SECRET_ACCESS_KEY`
    /// where absent.
    pub secret_access_key: Option<SecretRef>,
    /// The secret holding a session token, sent where it is set;
    /// `AWS_SESSION_TOKEN` where absent.
    pub session_token: Option<SecretRef>,
    /// The secret holding a key to seal every record with, such as
    /// `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt
    /// (ADR-0005); records are not sealed where absent.
    pub key: Option<SecretRef>,
}

fn named<'a>(given: Option<&'a SecretRef>, otherwise: &'a str) -> &'a str {
    given.map_or(otherwise, |named| named.secret.as_str())
}

impl StoreOptions {
    /// The table.
    #[must_use]
    pub fn table(&self) -> &str {
        self.table.as_deref().unwrap_or(TABLE)
    }

    /// The secret holding the access key's id.
    #[must_use]
    pub fn access_key_id(&self) -> &str {
        named(self.access_key_id.as_ref(), ACCESS_KEY_ID)
    }

    /// The secret holding the secret access key.
    #[must_use]
    pub fn secret_access_key(&self) -> &str {
        named(self.secret_access_key.as_ref(), SECRET_ACCESS_KEY)
    }

    /// The secret holding a session token.
    #[must_use]
    pub fn session_token(&self) -> &str {
        named(self.session_token.as_ref(), SESSION_TOKEN)
    }

    /// The name of every secret the store is reached with, named or not,
    /// so that each is hidden wherever herald writes.
    #[must_use]
    pub fn secrets(&self) -> [&str; 3] {
        [
            self.access_key_id(),
            self.secret_access_key(),
            self.session_token(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_option_is_the_one_named_or_else_what_aws_sets() {
        let defaults = StoreOptions::default();
        assert_eq!(
            (
                defaults.table(),
                defaults.access_key_id(),
                defaults.secret_access_key(),
                defaults.session_token()
            ),
            (
                "herald",
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
                "AWS_SESSION_TOKEN"
            )
        );
        assert_eq!(
            defaults.secrets(),
            [
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
                "AWS_SESSION_TOKEN"
            ]
        );
        let named: StoreOptions = serde_json::from_str(
            r#"{"table": "records", "access_key_id": {"secret": "ID"}, "secret_access_key": {"secret": "KEY"}, "session_token": {"secret": "TOKEN"}}"#,
        )
        .unwrap();
        assert_eq!(
            (
                named.table(),
                named.access_key_id(),
                named.secret_access_key(),
                named.session_token()
            ),
            ("records", "ID", "KEY", "TOKEN")
        );
        assert_eq!(named.secrets(), ["ID", "KEY", "TOKEN"]);
    }
}
