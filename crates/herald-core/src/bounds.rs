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

impl Default for Sandbox {
    fn default() -> Self {
        Self::DEFAULT
    }
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

impl Default for Inbound {
    fn default() -> Self {
        Self::DEFAULT
    }
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
            "description": "How many pages of its own posts the destination reads back, 5 by default.",
            "type": "integer",
            "minimum": 1,
            "maximum": Self::MOST.0
        })
    }
}
