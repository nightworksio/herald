use std::fmt;

use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

/// A length of time in a config, as `250ms`, `45s`, `30m`, `12h`, `14d` or
/// `2w`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duration(pub SignedDuration);

impl JsonSchema for Duration {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Duration".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A length of time: a number and ms, s, m, h, d or w, as \"14d\".",
            "type": "string",
            "pattern": Self::PATTERN
        })
    }
}

const SECOND: i64 = 1;
const MINUTE: i64 = 60;
const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;
const WEEK: i64 = 604_800;

impl Duration {
    /// The pattern the schema holds a length of time to: a number above 0
    /// and its unit.
    pub const PATTERN: &str = "^0*[1-9][0-9]*(ms|[smhdw])$";

    fn parse(text: &str) -> Option<Self> {
        if let Some(number) = text.strip_suffix("ms") {
            return digits(number)
                .filter(|millis| *millis > 0)
                .map(|millis| Self(SignedDuration::from_millis(millis)));
        }
        let unit = text.chars().last()?;
        let number = digits(text.get(..text.len() - 1)?)?;
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

/// The number `text` writes in decimal digits alone, with no sign.
fn digits(text: &str) -> Option<i64> {
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

struct DurationVisitor;

impl serde::de::Visitor<'_> for DurationVisitor {
    type Value = Duration;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "a length of time such as \"250ms\", \"30s\", \"30m\", \"12h\", \"14d\" or \"2w\"",
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_is_a_number_and_a_unit() {
        let cases = [
            ("45s", 45),
            ("30m", 1_800),
            ("12h", 43_200),
            ("14d", 1_209_600),
            ("2w", 1_209_600),
        ];
        assert_eq!(
            Duration::parse("250ms"),
            Some(Duration(SignedDuration::from_millis(250)))
        );
        for text in ["ms", "0ms", "-5ms", "+5ms", "1.5ms"] {
            assert_eq!(Duration::parse(text), None, "{text}");
        }
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
            "+5m",
            "1.5h",
            "99999999999999999w",
        ] {
            assert_eq!(Duration::parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_duration_is_read_from_its_text_and_a_wrong_one_says_what_was_expected() {
        assert_eq!(
            serde_json::from_str::<Duration>(r#""2w""#).unwrap(),
            Duration(SignedDuration::from_secs(1_209_600))
        );
        assert_eq!(
            serde_json::from_str::<Duration>(r#""2y""#)
                .unwrap_err()
                .to_string(),
            "invalid value: string \"2y\", expected a length of time such as \"250ms\", \"30s\", \"30m\", \"12h\", \"14d\" or \"2w\" at line 1 column 4"
        );
    }

    #[test]
    fn the_schema_holds_a_duration_to_its_pattern() {
        let schema = schemars::schema_for!(Duration);
        assert_eq!(
            schema.get("pattern").and_then(|pattern| pattern.as_str()),
            Some("^0*[1-9][0-9]*(ms|[smhdw])$")
        );
        assert_eq!(
            schema.get("type").and_then(|kind| kind.as_str()),
            Some("string")
        );
    }
}
