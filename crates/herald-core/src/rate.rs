use std::fmt;
use std::num::NonZeroU32;

use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

/// How many of something a config allows in a length of time, as `120/m`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    /// How many.
    pub count: NonZeroU32,
    /// In how long.
    pub per: Per,
}

/// The length of time a [`Rate`] counts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Per {
    /// A second, `s`.
    Second,
    /// A minute, `m`.
    Minute,
    /// An hour, `h`.
    Hour,
    /// A day, `d`.
    Day,
}

impl Per {
    const ALL: [Self; 4] = [Self::Second, Self::Minute, Self::Hour, Self::Day];

    /// Its unit, as a rate writes it.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Second => "s",
            Self::Minute => "m",
            Self::Hour => "h",
            Self::Day => "d",
        }
    }

    /// How long it is.
    #[must_use]
    pub const fn length(self) -> SignedDuration {
        match self {
            Self::Second => SignedDuration::from_secs(1),
            Self::Minute => SignedDuration::from_mins(1),
            Self::Hour => SignedDuration::from_hours(1),
            Self::Day => SignedDuration::from_hours(24),
        }
    }
}

impl JsonSchema for Rate {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Rate".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "How many in a length of time: a number, a slash and s, m, h or d, as \"120/m\".",
            "type": "string",
            "pattern": Self::PATTERN
        })
    }
}

impl Rate {
    /// The pattern the schema holds a rate to: a number above 0, a slash
    /// and its unit.
    pub(crate) const PATTERN: &str = "^0*[1-9][0-9]*/[smhd]$";

    /// `count` in each `per`.
    #[must_use]
    pub const fn new(count: NonZeroU32, per: Per) -> Self {
        Self { count, per }
    }

    fn parse(text: &str) -> Option<Self> {
        let (number, unit) = text.split_once('/')?;
        let per = Per::ALL.into_iter().find(|per| per.unit() == unit)?;
        if !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let count = NonZeroU32::new(number.parse().ok()?)?;
        Some(Self { count, per })
    }

    /// Whether it allows more than `other` over any length of time.
    #[must_use]
    pub fn exceeds(&self, other: &Self) -> bool {
        let mine = i64::from(self.count.get()) * other.per.length().as_secs();
        let theirs = i64::from(other.count.get()) * self.per.length().as_secs();
        mine > theirs
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.count, self.per.unit())
    }
}

struct RateVisitor;

impl serde::de::Visitor<'_> for RateVisitor {
    type Value = Rate;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a rate such as \"120/m\", \"5/s\", \"1000/h\" or \"10000/d\"")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Rate, E> {
        Rate::parse(text).ok_or_else(|| E::invalid_value(serde::de::Unexpected::Str(text), &self))
    }
}

impl<'de> Deserialize<'de> for Rate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(RateVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(count: u32, per: Per) -> Rate {
        Rate::new(NonZeroU32::new(count).unwrap(), per)
    }

    #[test]
    fn a_rate_is_a_number_a_slash_and_a_unit() {
        let cases = [
            ("5/s", rate(5, Per::Second)),
            ("120/m", rate(120, Per::Minute)),
            ("007/h", rate(7, Per::Hour)),
            ("10000/d", rate(10_000, Per::Day)),
        ];
        for (text, read) in cases {
            assert_eq!(Rate::parse(text), Some(read), "{text}");
        }
        for text in [
            "",
            "/m",
            "120",
            "120/",
            "0/m",
            "-1/m",
            "+1/m",
            "1.5/m",
            "120/w",
            "120/ms",
            "120/mm",
            "5000000000/s",
            "120 /m",
        ] {
            assert_eq!(Rate::parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_rate_is_written_as_it_is_read() {
        for text in ["5/s", "120/m", "7/h", "10000/d"] {
            assert_eq!(Rate::parse(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn a_rate_exceeds_another_only_where_it_allows_more_in_the_same_time() {
        let per_minute = rate(120, Per::Minute);
        assert!(rate(3, Per::Second).exceeds(&per_minute));
        assert!(!rate(2, Per::Second).exceeds(&per_minute));
        assert!(!per_minute.exceeds(&rate(2, Per::Second)));
        assert!(!rate(7_200, Per::Hour).exceeds(&per_minute));
        assert!(rate(7_201, Per::Hour).exceeds(&per_minute));
        assert!(per_minute.exceeds(&rate(7_199, Per::Hour)));
        assert!(!rate(172_800, Per::Day).exceeds(&per_minute));
        assert!(rate(172_801, Per::Day).exceeds(&per_minute));
    }

    #[test]
    fn a_rate_is_read_from_its_text_and_a_wrong_one_says_what_was_expected() {
        assert_eq!(
            serde_json::from_str::<Rate>(r#""120/m""#).unwrap(),
            rate(120, Per::Minute)
        );
        let error = serde_json::from_str::<Rate>(r#""120/w""#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("a rate such as \"120/m\""), "{error}");
    }

    #[test]
    fn the_schema_holds_a_rate_to_its_pattern() {
        let schema = serde_json::to_value(schemars::schema_for!(Rate)).unwrap();
        assert_eq!(schema["pattern"], Rate::PATTERN);
    }
}
