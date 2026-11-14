use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

/// A number of bytes in a config, as `512KiB` or `1MiB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Size(pub usize);

impl JsonSchema for Size {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Size".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A number of bytes: a number and KiB or MiB, as \"1MiB\".",
            "type": "string",
            "pattern": Self::PATTERN
        })
    }
}

impl Size {
    /// The pattern the schema holds a size to: a number above 0 and its
    /// unit.
    pub(crate) const PATTERN: &str = "^0*[1-9][0-9]*(KiB|MiB)$";
    /// The bytes in a KiB.
    pub(crate) const KIB: usize = 1 << 10;
    /// The bytes in a MiB.
    pub(crate) const MIB: usize = 1 << 20;

    fn parse(text: &str) -> Option<Self> {
        let (number, unit) = if let Some(number) = text.strip_suffix("KiB") {
            (number, Self::KIB)
        } else {
            (text.strip_suffix("MiB")?, Self::MIB)
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

    /// `bytes` as a config writes them, in quotes: in MiB where they are
    /// whole MiB, and in KiB otherwise.
    #[must_use]
    pub fn written(bytes: usize) -> String {
        if bytes.is_multiple_of(Self::MIB) {
            format!("\"{}MiB\"", bytes / Self::MIB)
        } else {
            format!("\"{}KiB\"", bytes / Self::KIB)
        }
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
    fn a_size_is_read_from_a_string_and_anything_else_refused_with_what_it_takes() {
        assert_eq!(
            serde_json::from_str::<Size>("\"2KiB\"").ok(),
            Some(Size(2 << 10))
        );
        for written in ["1024", "\"2KB\""] {
            let refused = serde_json::from_str::<Size>(written).unwrap_err();
            assert!(
                refused
                    .to_string()
                    .contains("expected a number of bytes such as \"512KiB\" or \"1MiB\""),
                "{refused}"
            );
        }
    }

    #[test]
    fn a_size_s_schema_is_a_string_of_a_number_and_a_binary_unit() {
        let schema = serde_json::to_value(schemars::schema_for!(Size)).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["pattern"], "^0*[1-9][0-9]*(KiB|MiB)$");
    }

    #[test]
    fn a_size_is_written_in_whole_mib_or_kib() {
        assert_eq!(Size::written(4 << 20), "\"4MiB\"");
        assert_eq!(Size::written(256 << 10), "\"256KiB\"");
    }
}
