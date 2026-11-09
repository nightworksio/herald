use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, de};

/// An embed's colour, as a config writes it: `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour(pub u32);

impl Colour {
    fn parse(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('#')?;
        if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        u32::from_str_radix(digits, 16).ok().map(Self)
    }
}

impl fmt::Display for Colour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:06x}", self.0)
    }
}

impl<'de> Deserialize<'de> for Colour {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            de::Error::custom(format!(
                "\"{text}\" is not a colour; write it as \"#rrggbb\", such as \"#e39a26\""
            ))
        })
    }
}

impl JsonSchema for Colour {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Colour".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A colour, written \"#rrggbb\".",
            "type": "string",
            "pattern": "^#[0-9A-Fa-f]{6}$"
        })
    }
}

/// The colours of an announcement's embed, each where the config sets one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Colours {
    /// A release's colour; the gold of herald's trumpet, `#e39a26`, where
    /// unset.
    pub release: Option<Colour>,
    /// A pre-release's colour; the violet of herald's banner, `#8f88c7`,
    /// where unset.
    pub prerelease: Option<Colour>,
}

/// How long a quiet thread stays open before Discord archives it: one of the
/// four lengths Discord takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
pub enum ArchiveAfter {
    /// An hour.
    #[serde(rename = "1h")]
    Hour,
    /// A day.
    #[serde(rename = "1d")]
    Day,
    /// Three days.
    #[serde(rename = "3d")]
    ThreeDays,
    /// A week, the longest Discord keeps a quiet thread open.
    #[default]
    #[serde(rename = "1w")]
    Week,
}

impl ArchiveAfter {
    /// The length in minutes, as Discord's `auto_archive_duration` takes it.
    #[must_use]
    pub const fn minutes(self) -> u32 {
        match self {
            Self::Hour => 60,
            Self::Day => 1_440,
            Self::ThreeDays => 4_320,
            Self::Week => 10_080,
        }
    }
}

/// How a Discord destination's posts look: its embeds' colours, and how
/// long its threads stay open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    /// A release's embed colour.
    pub release: Colour,
    /// A pre-release's embed colour.
    pub prerelease: Colour,
    /// How long a quiet thread stays open.
    pub archive_after: ArchiveAfter,
}

impl Look {
    /// The look where a config sets none.
    pub const DEFAULT: Self = Self {
        release: Colour(0x00E3_9A26),
        prerelease: Colour(0x008F_88C7),
        archive_after: ArchiveAfter::Week,
    };

    /// The look `colours` and `archive_after` give, today's where unset.
    #[must_use]
    pub fn of(colours: Option<Colours>, archive_after: Option<ArchiveAfter>) -> Self {
        let colours = colours.unwrap_or_default();
        Self {
            release: colours.release.unwrap_or(Self::DEFAULT.release),
            prerelease: colours.prerelease.unwrap_or(Self::DEFAULT.prerelease),
            archive_after: archive_after.unwrap_or(Self::DEFAULT.archive_after),
        }
    }
}

impl Default for Look {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read<T: for<'de> Deserialize<'de>>(written: &str) -> Result<T, String> {
        serde_json::from_str(written).map_err(|error| error.to_string())
    }

    #[test]
    fn a_colour_is_written_as_a_hash_and_six_hex_digits() {
        assert_eq!(read::<Colour>("\"#E39a26\""), Ok(Colour(0x00E3_9A26)));
        assert_eq!(Colour(0x00E3_9A26).to_string(), "#e39a26");
        assert_eq!(Colour(0x0000_00ff).to_string(), "#0000ff");
        for written in ["e39a26", "#e39a2", "#e39a266", "#e39a2g", "#+39a26"] {
            let refused = read::<Colour>(&format!("\"{written}\"")).unwrap_err();
            assert!(
                refused.contains(&format!("\"{written}\" is not a colour")),
                "{refused}"
            );
        }
    }

    #[test]
    fn the_schema_holds_a_colour_to_its_pattern() {
        let schema = schemars::schema_for!(Colour);
        assert_eq!(
            schema.get("pattern").and_then(|pattern| pattern.as_str()),
            Some("^#[0-9A-Fa-f]{6}$")
        );
        assert_eq!(
            schema.get("type").and_then(|kind| kind.as_str()),
            Some("string")
        );
    }

    #[test]
    fn a_thread_stays_open_for_one_of_discord_s_four_lengths() {
        for (written, minutes) in [("1h", 60), ("1d", 1_440), ("3d", 4_320), ("1w", 10_080)] {
            assert_eq!(
                read::<ArchiveAfter>(&format!("\"{written}\"")).map(ArchiveAfter::minutes),
                Ok(minutes)
            );
        }
        assert!(read::<ArchiveAfter>("\"2d\"").is_err());
        assert_eq!(ArchiveAfter::default(), ArchiveAfter::Week);
    }

    #[test]
    fn a_look_takes_what_the_config_sets_and_today_s_look_elsewhere() {
        assert_eq!(Look::of(None, None), Look::DEFAULT);
        assert_eq!(Look::default(), Look::DEFAULT);
        let set = Look::of(
            Some(Colours {
                release: Some(Colour(1)),
                prerelease: None,
            }),
            Some(ArchiveAfter::Hour),
        );
        assert_eq!(
            set,
            Look {
                release: Colour(1),
                prerelease: Look::DEFAULT.prerelease,
                archive_after: ArchiveAfter::Hour,
            }
        );
        assert_eq!(
            Look::of(
                Some(Colours {
                    release: None,
                    prerelease: Some(Colour(2)),
                }),
                None,
            )
            .prerelease,
            Colour(2)
        );
    }
}
