//! What every store that keeps records reads beside its kind's own
//! options.

use herald_core::{Attempts, Duration};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};
use toml::Spanned;

/// What every store that keeps records reads beside its kind's own
/// options: how long it keeps them, and how many times it tries a write.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordsFile {
    /// How long a webhook delivery's id is kept after it was admitted, so a
    /// replay of it is refused (ADR-0013); "7d" where absent, and at least
    /// "3d", longer than any sender goes on retrying a delivery.
    #[serde(rename = "keep_deliveries")]
    #[schemars(with = "Option<Duration>")]
    pub(crate) deliveries: Option<Spanned<Duration>>,
    /// How long a ledger entry, and where each of its deliveries to a
    /// follower's inbox stands, is kept after a run last claimed it; "90d"
    /// where absent.
    #[serde(rename = "keep_ledger")]
    pub(crate) ledger: Option<Keep>,
    /// How long a Note herald's ActivityPub actors published is kept, and
    /// served by its id; "forever" where absent.
    #[serde(rename = "keep_notes")]
    pub(crate) notes: Option<Keep>,
    /// How long what a model answered a task is kept, and reused while
    /// the pull request it decided of is unchanged; "30d" where absent.
    #[serde(rename = "keep_decisions")]
    pub(crate) decisions: Option<Keep>,
    /// How long a pull request kept mode keeps is kept after it closed;
    /// "30d" where absent (ADR-0061).
    #[serde(rename = "keep_pulls")]
    pub(crate) pulls: Option<Keep>,
    /// How many times a write that lost a race to an unrelated write is
    /// tried.
    pub(crate) attempts: Option<Attempts>,
}

impl RecordsFile {
    /// Its keys, as the config spells them.
    pub(crate) const KEYS: [&str; 6] = [
        "keep_deliveries",
        "keep_ledger",
        "keep_notes",
        "keep_decisions",
        "keep_pulls",
        "attempts",
    ];
}

/// How long a store keeps a kind of record: a length of time, or for ever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keep {
    /// For ever.
    Forever,
    /// For this long.
    For(Duration),
}

impl Keep {
    const FOREVER: &str = "forever";

    /// How long, or `None` for ever.
    pub(crate) const fn kept(self) -> Option<jiff::SignedDuration> {
        match self {
            Self::Forever => None,
            Self::For(duration) => Some(duration.0),
        }
    }
}

impl JsonSchema for Keep {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Keep".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A length of time, or \"forever\".",
            "anyOf": [generator.subschema_for::<Duration>(), { "const": Self::FOREVER }]
        })
    }
}

impl<'de> Deserialize<'de> for Keep {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text == Self::FOREVER {
            return Ok(Self::Forever);
        }
        Duration::deserialize(serde::de::value::StrDeserializer::<D::Error>::new(&text))
            .map(Self::For)
            .map_err(|_: D::Error| {
                serde::de::Error::invalid_value(
                    serde::de::Unexpected::Str(&text),
                    &"a length of time such as \"90d\", or \"forever\"",
                )
            })
    }
}
