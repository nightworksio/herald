use std::fmt;
use std::num::NonZeroUsize;

use herald_core::{Kind, Pinning, Prereleases, SecretRef};
use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};
use toml::Spanned;

/// A length of time, as `30m`, `12h`, `14d` or `2w`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Duration(pub(crate) SignedDuration);

impl JsonSchema for Duration {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Duration".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A length of time: a number and m, h, d or w, as \"14d\".",
            "type": "string",
            "pattern": "^[0-9]+[mhdw]$"
        })
    }
}

const SECOND: i64 = 1;
const MINUTE: i64 = 60;
const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;
const WEEK: i64 = 604_800;

impl Duration {
    fn parse(text: &str) -> Option<Self> {
        let unit = text.chars().last()?;
        let number: i64 = text.get(..text.len() - 1)?.parse().ok()?;
        let seconds = match unit {
            's' => SECOND,
            'm' => MINUTE,
            'h' => HOUR,
            'd' => DAY,
            'w' => WEEK,
            _ => return None,
        };
        number
            .checked_mul(seconds)
            .filter(|seconds| *seconds > 0)
            .map(|seconds| Self(SignedDuration::from_secs(seconds)))
    }
}

struct DurationVisitor;

impl serde::de::Visitor<'_> for DurationVisitor {
    type Value = Duration;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a length of time such as \"30s\", \"30m\", \"12h\", \"14d\" or \"2w\"")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Duration, E> {
        Duration::parse(text)
            .ok_or_else(|| E::invalid_value(serde::de::Unexpected::Str(text), &self))
    }
}

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(DurationVisitor)
    }
}

/// A number of bytes, as `512KiB` or `1MiB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Size(pub(crate) usize);

impl JsonSchema for Size {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Size".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A number of bytes: a number and KiB or MiB, as \"1MiB\".",
            "type": "string",
            "pattern": "^[0-9]+(KiB|MiB)$"
        })
    }
}

const KIB: usize = 1 << 10;
const MIB: usize = 1 << 20;

impl Size {
    fn parse(text: &str) -> Option<Self> {
        let (number, unit) = if let Some(number) = text.strip_suffix("KiB") {
            (number, KIB)
        } else {
            (text.strip_suffix("MiB")?, MIB)
        };
        if !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        number
            .parse::<usize>()
            .ok()?
            .checked_mul(unit)
            .filter(|bytes| *bytes > 0)
            .map(Self)
    }
}

struct SizeVisitor;

impl serde::de::Visitor<'_> for SizeVisitor {
    type Value = Size;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a number of bytes such as \"512KiB\" or \"1MiB\"")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Size, E> {
        Size::parse(text).ok_or_else(|| E::invalid_value(serde::de::Unexpected::Str(text), &self))
    }
}

impl<'de> Deserialize<'de> for Size {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(SizeVisitor)
    }
}

/// A section that takes no options beside its kind.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename = "Options")]
pub(crate) struct NoOptions {}

/// `[markers]`: the installation's marker key (ADR-0011).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Markers")]
pub(crate) struct MarkersFile {
    /// The secret holding the marker key, such as `HERALD_MARKER_SECRET`.
    pub(crate) key: SecretRef,
}

/// A thread a route opens, and when housekeeping closes it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Thread")]
pub(crate) struct ThreadFile {
    /// The template of the thread's name.
    #[schemars(with = "String")]
    pub(crate) name: Spanned<String>,
    /// How long after the announcement the thread is archived.
    pub(crate) archive_after: Option<Duration>,
    /// How long after the announcement the thread is locked, and archived.
    pub(crate) lock_after: Option<Duration>,
}

/// Who a route mentions.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Mentions")]
pub(crate) struct MentionsFile {
    /// The roles it pings, by the destination's ids for them.
    #[serde(default)]
    pub(crate) roles: Vec<String>,
    /// Whether it mentions the subscribers herald keeps.
    #[serde(default)]
    pub(crate) subscribers: bool,
}

/// What a route says whatever its destination.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Route")]
pub(crate) struct RouteFile {
    /// The route's stable id, part of every announcement's key.
    #[schemars(with = "String")]
    pub(crate) id: Spanned<String>,
    /// The sources it takes events from.
    #[schemars(with = "Vec<String>")]
    pub(crate) sources: Vec<Spanned<String>>,
    /// The kinds of event it takes; every kind where absent.
    #[serde(default)]
    pub(crate) kinds: Vec<Kind>,
    /// Which pre-releases it takes.
    #[serde(default)]
    pub(crate) prereleases: Prereleases,
    /// The versions it takes, as a pattern in which `*` stands for any run
    /// of characters and `?` for any one, such as `"*.*.*"`, which leaves
    /// out a Docker Hub tag such as `latest`; every version where absent.
    #[schemars(with = "Option<String>")]
    pub(crate) versions: Option<Spanned<String>>,
    /// The destination it announces to.
    #[schemars(with = "String")]
    pub(crate) destination: Spanned<String>,
    /// The template it renders with.
    #[schemars(with = "String")]
    pub(crate) template: Spanned<String>,
    /// Whether to crosspost each announcement.
    #[serde(default)]
    pub(crate) crosspost: bool,
    /// The thread to open for each announcement.
    pub(crate) thread: Option<ThreadFile>,
    /// Whether to pin each announcement in place of the route's earlier pin,
    /// and how the earlier pin is found.
    pub(crate) pin: Option<Pinning>,
    /// Who it mentions.
    #[serde(default)]
    pub(crate) mentions: MentionsFile,
}

/// The keys every route may give, whatever its destination.
pub(crate) const ROUTE_KEYS: [&str; 11] = [
    "id",
    "sources",
    "kinds",
    "prereleases",
    "versions",
    "destination",
    "template",
    "crosspost",
    "thread",
    "pin",
    "mentions",
];

/// A template: its title, and its body inline or in a file beside the config.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Template")]
pub(crate) struct TemplateFile {
    /// The title, on one line.
    #[schemars(with = "String")]
    pub(crate) title: Spanned<String>,
    /// The body, in Markdown.
    #[schemars(with = "Option<String>")]
    pub(crate) body: Option<Spanned<String>>,
    /// A file beside the config holding the body.
    #[schemars(with = "Option<String>")]
    pub(crate) file: Option<Spanned<String>>,
}

/// An app of the HTTP API (ADR-0007).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "App")]
pub(crate) struct AppFile {
    /// The SHA-256 of each of the app's tokens, written `sha256:` and 64
    /// lowercase hex digits, as `herald token new` prints it. A token is
    /// rotated by adding the new one before the old one is removed.
    #[serde(default)]
    #[schemars(with = "Vec<String>")]
    pub(crate) tokens: Vec<Spanned<String>>,
    /// The projects it may announce.
    #[serde(default)]
    pub(crate) projects: Vec<String>,
    /// The routes it announces to, each naming the app among its sources;
    /// every such route where it names none.
    #[serde(default)]
    #[schemars(with = "Vec<String>")]
    pub(crate) routes: Vec<Spanned<String>>,
}

/// How often a driver that keeps its own schedule polls and keeps house.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Schedule")]
pub(crate) struct ScheduleFile {
    /// How often sources are polled.
    pub(crate) poll: Option<Duration>,
    /// How often threads are kept.
    pub(crate) housekeep: Option<Duration>,
}

/// Where `herald server` keeps the keys inbound activities are checked with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[serde(rename = "KeyCache")]
pub(crate) enum KeyCacheFile {
    /// In memory, lost when the server stops.
    Memory,
    /// In the store, kept across restarts and shared by every instance.
    Store,
}

/// `[server.keys]`: the keys inbound activities are checked with.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Keys")]
pub(crate) struct KeysFile {
    /// Where they are kept: "memory", the default, or "store".
    #[schemars(with = "Option<KeyCacheFile>")]
    pub(crate) cache: Option<Spanned<KeyCacheFile>>,
    /// How long a key is used before its document is read again; 1h where
    /// absent.
    pub(crate) refresh_after: Option<Duration>,
    /// How long after a key is read an activity that does not hold under it
    /// is refused rather than having it read again; 1m where absent.
    pub(crate) reread_after: Option<Duration>,
    /// The most kept in memory, the one used longest ago forgotten first;
    /// 1024 where absent.
    pub(crate) most: Option<NonZeroUsize>,
}

/// `[server]`: how `herald server` listens, and what it takes on at once.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Server")]
pub(crate) struct ServerFile {
    /// The address and port it listens on; `--listen` and `HERALD_LISTEN`
    /// take its place. "127.0.0.1:8080" where absent, for a reverse proxy
    /// in front of it.
    #[schemars(with = "Option<String>")]
    pub(crate) listen: Option<Spanned<String>>,
    /// How many announcements it works on at once; a webhook arriving while
    /// as many more wait is answered 503. 8 where absent.
    pub(crate) jobs: Option<NonZeroUsize>,
    /// How many connections it holds at once; 256 where absent.
    pub(crate) connections: Option<NonZeroUsize>,
    /// The most an inbound request's body may hold; "1MiB" where absent.
    pub(crate) body: Option<Size>,
    /// How many actor documents it reads at once; 4 where absent.
    pub(crate) fetches: Option<NonZeroUsize>,
    /// How many autocompletions a session answers at once; one arriving
    /// while as many are answered is dropped. 4 where absent.
    pub(crate) completions: Option<NonZeroUsize>,
    /// How many of an announcement's inboxes are delivered to at once, by
    /// every command; 8 where absent.
    pub(crate) deliveries: Option<NonZeroUsize>,
    /// How long a session a destination holds hears nothing before it
    /// checks the connection is alive, and then as long again before it
    /// opens another; "30s" where absent.
    pub(crate) silence: Option<Duration>,
    /// The keys inbound activities are checked with.
    #[serde(default)]
    pub(crate) keys: KeysFile,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_a_number_and_a_binary_unit() {
        assert_eq!(Size::parse("512KiB"), Some(Size(512 * 1024)));
        assert_eq!(Size::parse("1MiB"), Some(Size(1024 * 1024)));
        for text in [
            "",
            "1",
            "1KB",
            "1kib",
            "0MiB",
            "+1MiB",
            "-1MiB",
            "1.5MiB",
            "MiB",
            "99999999999999999999MiB",
        ] {
            assert_eq!(Size::parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_duration_is_a_number_and_a_unit() {
        let cases = [
            ("45s", 45),
            ("30m", 1_800),
            ("12h", 43_200),
            ("14d", 1_209_600),
            ("2w", 1_209_600),
        ];
        for (text, seconds) in cases {
            assert_eq!(
                Duration::parse(text),
                Some(Duration(SignedDuration::from_secs(seconds))),
                "{text}"
            );
        }
        for text in [
            "",
            "d",
            "14",
            "14y",
            "0d",
            "-1d",
            "1.5h",
            "99999999999999999w",
        ] {
            assert_eq!(Duration::parse(text), None, "{text}");
        }
    }
}
