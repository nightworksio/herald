use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

/// How far a template may go as it renders: the fuel its instructions burn,
/// how deep its calls nest, and how many bytes it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sandbox {
    /// The fuel a render may burn.
    pub fuel: u64,
    /// How deep its calls and includes may nest.
    pub recursion: usize,
    /// The most bytes it may write.
    pub output: usize,
}

impl Sandbox {
    /// The sandbox where a config sets none.
    pub const DEFAULT: Self = Self {
        fuel: 100_000,
        recursion: 16,
        output: 262_144,
    };
    /// The widest sandbox a config may set: four times the default.
    pub const MOST: Self = Self {
        fuel: Self::DEFAULT.fuel * 4,
        recursion: Self::DEFAULT.recursion * 4,
        output: Self::DEFAULT.output * 4,
    };
}

/// What a signed inbound request may carry and still be checked: how far
/// its signing time may lie from now, how many signatures it carries, and
/// how many fields one HTTP signature covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inbound {
    /// How far a signed timestamp may lie from now, either way, before the
    /// request is taken for a replay.
    pub replay_window: SignedDuration,
    /// The most signatures a request may carry. Each is checked in turn, so
    /// a request with more is refused before any is checked; a sender
    /// rotating its keys signs with two or three.
    pub signatures: usize,
    /// The most fields one HTTP signature may cover.
    pub covered: usize,
    /// The most bytes of a key's or an actor's document read: far past any
    /// actor's, and small enough that a request naming a hostile key costs
    /// herald little.
    pub document: usize,
}

impl Inbound {
    /// The fewest headers `covered` may be: the four every ActivityPub
    /// signature covers, `(request-target)`, `host`, `date` and `digest`.
    pub const LEAST_COVERED: usize = 4;
    /// The bounds where a config sets none.
    pub const DEFAULT: Self = Self {
        replay_window: SignedDuration::from_mins(5),
        signatures: 4,
        covered: 16,
        document: 262_144,
    };
    /// The widest bounds a config may set: four times the default.
    pub const MOST: Self = Self {
        replay_window: SignedDuration::from_mins(20),
        signatures: Self::DEFAULT.signatures * 4,
        covered: Self::DEFAULT.covered * 4,
        document: Self::DEFAULT.document * 4,
    };
}

/// How many pages of its own posts a destination reads back for the markers
/// they carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReadBack(u8);

impl ReadBack {
    /// The pages read where a config sets none.
    pub const DEFAULT: Self = Self(5);
    /// The most pages a config may set: four times the default.
    pub const MOST: Self = Self(Self::DEFAULT.0 * 4);

    /// `pages` as a setting, where it lies from 1 to [`Self::MOST`].
    #[must_use]
    pub fn new(pages: u8) -> Option<Self> {
        (1..=Self::MOST.0).contains(&pages).then_some(Self(pages))
    }

    /// The pages read.
    #[must_use]
    pub fn pages(self) -> u8 {
        self.0
    }
}

impl Default for ReadBack {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl<'de> Deserialize<'de> for ReadBack {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pages = u8::deserialize(deserializer)?;
        Self::new(pages).ok_or_else(|| {
            D::Error::custom(format!(
                "read_back reads 1 to {} pages, not {pages}",
                Self::MOST.0
            ))
        })
    }
}

impl JsonSchema for ReadBack {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ReadBack".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": format!(
                "How many pages of its own posts the destination reads back, {} by default.",
                Self::DEFAULT.0
            ),
            "type": "integer",
            "minimum": 1,
            "maximum": Self::MOST.0
        })
    }
}

/// How many pins a destination looks up, one request each, as it finds a
/// route's earlier pins to take down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PinLookups(u8);

impl PinLookups {
    /// The pins looked up where a config sets none.
    pub const DEFAULT: Self = Self(25);
    /// The most a config may set: four times the default.
    pub const MOST: Self = Self(Self::DEFAULT.0 * 4);

    /// `pins` as a setting, where it lies from 1 to [`Self::MOST`].
    #[must_use]
    pub fn new(pins: u8) -> Option<Self> {
        (1..=Self::MOST.0).contains(&pins).then_some(Self(pins))
    }

    /// The pins looked up.
    #[must_use]
    pub fn pins(self) -> usize {
        usize::from(self.0)
    }

    /// What is said where `candidates` pins, each to be looked up, are
    /// more than it looks up: how to take every one into account.
    #[must_use]
    pub fn over(self, candidates: usize) -> Option<String> {
        let most = self.pins();
        (candidates > most).then(|| {
            let among = format!(
                "the route's earlier pins are among {candidates} pins, more than the {most} it looks up"
            );
            if self < Self::MOST {
                format!(
                    "{among}; raise pin_lookups, up to {}, or unpin them by hand",
                    Self::MOST.0
                )
            } else {
                format!("{among}; unpin them by hand")
            }
        })
    }
}

impl Default for PinLookups {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl<'de> Deserialize<'de> for PinLookups {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pins = u8::deserialize(deserializer)?;
        Self::new(pins).ok_or_else(|| {
            D::Error::custom(format!(
                "pin_lookups looks up 1 to {} pins, not {pins}",
                Self::MOST.0
            ))
        })
    }
}

impl JsonSchema for PinLookups {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PinLookups".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": format!(
                "How many pins the destination looks up, one request each, as it finds the route's earlier pins, {} by default.",
                Self::DEFAULT.0
            ),
            "type": "integer",
            "minimum": 1,
            "maximum": Self::MOST.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destination_looks_up_1_to_100_pins_and_25_by_default() {
        assert_eq!(PinLookups::default().pins(), 25);
        assert_eq!(PinLookups::MOST.pins(), 100);
        assert_eq!(PinLookups::new(3).map(PinLookups::pins), Some(3));
        assert_eq!(PinLookups::new(0), None);
        assert_eq!(PinLookups::new(101), None);
        assert_eq!(
            serde_json::from_str::<PinLookups>("7")
                .map(PinLookups::pins)
                .ok(),
            Some(7)
        );
        assert_eq!(
            serde_json::from_str::<PinLookups>("101")
                .unwrap_err()
                .to_string(),
            "pin_lookups looks up 1 to 100 pins, not 101"
        );
        assert_eq!(PinLookups::DEFAULT.over(25), None);
        assert_eq!(
            PinLookups::DEFAULT.over(26).as_deref(),
            Some(
                "the route's earlier pins are among 26 pins, more than the 25 it looks up; raise pin_lookups, up to 100, or unpin them by hand"
            )
        );
        assert_eq!(
            PinLookups::MOST.over(101).as_deref(),
            Some(
                "the route's earlier pins are among 101 pins, more than the 100 it looks up; unpin them by hand"
            )
        );
        let schema = serde_json::to_value(schemars::schema_for!(PinLookups)).unwrap();
        assert_eq!(schema["minimum"], 1);
        assert_eq!(schema["maximum"], 100);
        assert_eq!(
            schema["description"],
            "How many pins the destination looks up, one request each, as it finds the route's earlier pins, 25 by default."
        );
    }

    #[test]
    fn a_destination_reads_back_1_to_20_pages_and_5_by_default() {
        assert_eq!(ReadBack::default().pages(), 5);
        assert_eq!(ReadBack::MOST.pages(), 20);
        assert_eq!(ReadBack::new(3).map(ReadBack::pages), Some(3));
        assert_eq!(ReadBack::new(0), None);
        assert_eq!(ReadBack::new(21), None);
    }

    #[test]
    fn read_back_is_read_as_its_pages_and_refused_outside_them() {
        assert_eq!(
            serde_json::from_str::<ReadBack>("7")
                .map(ReadBack::pages)
                .ok(),
            Some(7)
        );
        let refused = serde_json::from_str::<ReadBack>("21").unwrap_err();
        assert_eq!(refused.to_string(), "read_back reads 1 to 20 pages, not 21");
    }

    #[test]
    fn read_back_s_schema_bounds_it_and_names_its_default() {
        let schema = serde_json::to_value(schemars::schema_for!(ReadBack)).unwrap();
        assert_eq!(schema["type"], "integer");
        assert_eq!(schema["minimum"], 1);
        assert_eq!(schema["maximum"], 20);
        assert_eq!(
            schema["description"],
            "How many pages of its own posts the destination reads back, 5 by default."
        );
    }
}
