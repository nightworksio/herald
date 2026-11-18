use std::collections::BTreeSet;

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use toml_datetime::Datetime;

/// What a peer may be granted to do with a resource of this herald.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    /// Push envelopes and tombstones into a `herald` source's inbox.
    Push,
}

impl Action {
    /// Its name, as the config writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Push => "push",
        }
    }
}

/// What a peer may do with one resource: its actions, until when, and the
/// identities it may give as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// What the peer may do, such as `["push"]`.
    pub actions: BTreeSet<Action>,
    /// When the grant ends, in RFC 3339 such as "2027-01-01T00:00:00Z" or
    /// as a TOML datetime with its offset; never where absent.
    #[serde(default, deserialize_with = "until")]
    #[schemars(with = "Option<String>")]
    pub until: Option<Timestamp>,
    /// The identities the peer may give as they are, each the start of
    /// identities up to a '/', such as "github.com/123456"; every other is
    /// taken as herald/<peer>/<identity>. None where absent.
    #[serde(default)]
    pub namespaces: BTreeSet<String>,
}

impl Grant {
    /// Whether it lets its peer do `action` at `now`.
    #[must_use]
    pub fn allows(&self, action: Action, now: Timestamp) -> bool {
        self.actions.contains(&action) && self.until.is_none_or(|until| now < until)
    }
}

/// How a grant's end is written: RFC 3339 text, or a TOML datetime.
#[derive(Deserialize)]
#[serde(untagged)]
enum Until {
    Text(String),
    Datetime(Datetime),
}

/// The instant a grant ends, from RFC 3339 text or a TOML datetime that
/// names its date, time and offset.
///
/// # Errors
///
/// It is written as neither, or names no instant.
pub fn until<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Timestamp>, D::Error> {
    let text = match Until::deserialize(deserializer) {
        Ok(Until::Text(text)) => text,
        Ok(Until::Datetime(datetime)) => datetime.to_string(),
        Err(_) => return Err(D::Error::custom(UNTIL)),
    };
    text.parse()
        .map(Some)
        .map_err(|_| D::Error::custom(format!("{text} is no instant; {UNTIL}")))
}

/// What `until` takes.
const UNTIL: &str =
    "until is a time with its offset, such as \"2027-01-01T00:00:00Z\" or 2027-01-01T00:00:00Z";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_until_written_as_neither_text_nor_a_datetime_is_refused() {
        let refused =
            serde_json::from_str::<Grant>(r#"{ "actions": ["push"], "until": 5 }"#).unwrap_err();
        assert!(refused.to_string().starts_with(UNTIL), "{refused}");
    }

    #[test]
    fn a_grant_allows_its_actions_only_before_it_ends() {
        let at = |second: i64| Timestamp::from_second(second).unwrap();
        let ending = Grant {
            actions: BTreeSet::from([Action::Push]),
            until: Some(at(10)),
            namespaces: BTreeSet::new(),
        };
        assert!(ending.allows(Action::Push, at(9)));
        assert!(!ending.allows(Action::Push, at(10)));
        let open = Grant {
            until: None,
            ..ending.clone()
        };
        assert!(open.allows(Action::Push, at(1_000_000)));
        assert!(!Grant::default().allows(Action::Push, at(0)));
        assert_eq!(Action::Push.name(), "push");
    }

    #[test]
    fn a_grant_reads_from_its_table_and_refuses_an_unknown_action() {
        let read: Grant = serde_json::from_value(serde_json::json!({
            "actions": ["push", "push"],
            "until": "2027-01-01T00:00:00Z",
            "namespaces": ["github.com/123456"],
        }))
        .unwrap();
        assert_eq!(read.actions, BTreeSet::from([Action::Push]));
        assert_eq!(read.until, Some("2027-01-01T00:00:00Z".parse().unwrap()));
        assert!(
            serde_json::from_value::<Grant>(serde_json::json!({ "actions": ["pull"] })).is_err()
        );
    }

    #[test]
    fn a_grant_ends_at_rfc_3339_text_naming_its_offset() {
        let read = |until: &str| {
            serde_json::from_value::<Grant>(
                serde_json::json!({ "actions": ["push"], "until": until }),
            )
            .map(|grant| grant.until)
            .map_err(|error| error.to_string())
        };
        assert_eq!(
            serde_json::from_value::<Grant>(serde_json::json!({ "actions": ["push"], "until": 5 }))
                .map_err(|error| error.to_string()),
            Err(UNTIL.to_owned())
        );
        assert_eq!(
            read("2027-01-01T01:00:00+01:00"),
            Ok(Some("2027-01-01T00:00:00Z".parse().unwrap()))
        );
        assert_eq!(read("2027-01-01"), Err("2027-01-01 is no instant; until is a time with its offset, such as \"2027-01-01T00:00:00Z\" or 2027-01-01T00:00:00Z".to_owned()));
    }
}
