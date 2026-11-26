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
    pub(crate) const PATTERN: &str = "^0*[1-9][0-9]*(ms|[smhdw])$";

    /// The length of time `text` writes, such as "7d", where it is one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
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

    /// The length of time as a config writes it, in its largest whole
    /// unit, such as "90d"; `parse` reads it back.
    #[must_use]
    pub fn written(self) -> String {
        let millis = self.0.as_millis();
        let seconds = self.0.as_secs();
        if millis % 1000 != 0 {
            return format!("{millis}ms");
        }
        let (count, unit) = [(WEEK, 'w'), (DAY, 'd'), (HOUR, 'h'), (MINUTE, 'm')]
            .into_iter()
            .find(|(unit, _)| seconds % unit == 0)
            .map_or((seconds, 's'), |(length, unit)| (seconds / length, unit));
        format!("{count}{unit}")
    }
}

/// The number `text` writes in decimal digits alone, with no sign.
fn digits(text: &str) -> Option<i64> {
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// What reads a length of time from a config's text: how it parses one,
/// and the lengths it takes, as an error says them.
struct Written<T>(fn(&str) -> Option<T>, &'static str);

impl<T> serde::de::Visitor<'_> for Written<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.1)
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<T, E> {
        (self.0)(text).ok_or_else(|| E::invalid_value(serde::de::Unexpected::Str(text), &self))
    }
}

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(Written(
            Self::parse,
            "a length of time such as \"250ms\", \"30s\", \"30m\", \"12h\", \"14d\" or \"2w\"",
        ))
    }
}

/// A length of time in a config that may be none at all, as `0s`, or as
/// a [`Duration`] writes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lasting(pub SignedDuration);

impl Lasting {
    /// The pattern the schema holds a length of time that may be none to:
    /// a number and its unit.
    pub(crate) const PATTERN: &str = "^[0-9]+(ms|[smhdw])$";

    /// The length of time `text` writes, such as "10s" or "0s", where it
    /// is one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let none = text
            .strip_suffix("ms")
            .or_else(|| text.strip_suffix(['s', 'm', 'h', 'd', 'w']))
            .is_some_and(|number| !number.is_empty() && number.bytes().all(|byte| byte == b'0'));
        if none {
            return Some(Self(SignedDuration::ZERO));
        }
        Duration::parse(text).map(|duration| Self(duration.0))
    }

    /// The length of time as a config writes it: "0s" for none, and else
    /// as a [`Duration`] writes it; `parse` reads it back.
    #[must_use]
    pub fn written(self) -> String {
        if self.0.is_zero() {
            "0s".to_owned()
        } else {
            Duration(self.0).written()
        }
    }
}

impl JsonSchema for Lasting {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Lasting".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A length of time, none at all among them: a number and ms, s, m, h, d or w, as \"10s\" or \"0s\".",
            "type": "string",
            "pattern": Self::PATTERN
        })
    }
}

impl<'de> Deserialize<'de> for Lasting {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(Written(
            Self::parse,
            "a length of time such as \"0s\", \"250ms\", \"10s\" or \"1m\"",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_is_written_in_its_largest_whole_unit_and_read_back() {
        for text in ["250ms", "45s", "30m", "12h", "90d", "2w", "1500ms", "61s"] {
            let duration = Duration::parse(text).unwrap();
            assert_eq!(duration.written(), text);
            assert_eq!(Duration::parse(&duration.written()), Some(duration));
        }
        assert_eq!(Duration(SignedDuration::from_hours(48)).written(), "2d");
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

    #[test]
    fn a_lasting_is_none_at_all_or_a_duration() {
        for text in ["0s", "0ms", "00m", "0h", "0d", "0w"] {
            assert_eq!(
                Lasting::parse(text),
                Some(Lasting(SignedDuration::ZERO)),
                "{text}"
            );
        }
        assert_eq!(
            Lasting::parse("10s"),
            Some(Lasting(SignedDuration::from_secs(10)))
        );
        for text in ["", "s", "ms", "0", "0y", "-0s", "+0s", "0.0s", "1.5s", "10"] {
            assert_eq!(Lasting::parse(text), None, "{text}");
        }
        assert_eq!(
            serde_json::from_str::<Lasting>(r#""0s""#).unwrap(),
            Lasting(SignedDuration::ZERO)
        );
        assert_eq!(
            serde_json::from_str::<Lasting>(r#""-1s""#)
                .unwrap_err()
                .to_string(),
            "invalid value: string \"-1s\", expected a length of time such as \"0s\", \"250ms\", \"10s\" or \"1m\" at line 1 column 5"
        );
        for (text, written) in [
            ("00m", "0s"),
            ("0w", "0s"),
            ("60s", "1m"),
            ("250ms", "250ms"),
        ] {
            let lasting = Lasting::parse(text).unwrap();
            assert_eq!(lasting.written(), written, "{text}");
            assert_eq!(Lasting::parse(written), Some(lasting), "{text}");
        }
        let schema = schemars::schema_for!(Lasting);
        assert_eq!(
            schema.get("pattern").and_then(|pattern| pattern.as_str()),
            Some("^[0-9]+(ms|[smhdw])$")
        );
    }
}
