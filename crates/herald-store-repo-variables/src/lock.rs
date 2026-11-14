use herald_core::{Count, Counted, Duration};
use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

/// How long the store's lock holds before it is taken as abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockLease(SignedDuration);

impl LockLease {
    /// The lease where a config sets none.
    pub const DEFAULT: Self = Self(SignedDuration::from_mins(10));
    /// The longest a config may set: four times the default.
    pub const MOST: Self = Self(SignedDuration::from_mins(40));

    /// `lease` as a setting, where it is no longer than [`Self::MOST`].
    #[must_use]
    pub fn new(lease: SignedDuration) -> Option<Self> {
        (lease <= Self::MOST.0).then_some(Self(lease))
    }

    /// The lease.
    #[must_use]
    pub const fn get(self) -> SignedDuration {
        self.0
    }
}

impl Default for LockLease {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl<'de> Deserialize<'de> for LockLease {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Duration(lease) = Duration::deserialize(deserializer)?;
        Self::new(lease).ok_or_else(|| {
            D::Error::custom(format!(
                "lock_lease holds at most \"{}m\", four times its default, not {lease:#}",
                Self::MOST.0.as_mins()
            ))
        })
    }
}

impl JsonSchema for LockLease {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "LockLease".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = Duration::json_schema(generator);
        schema.insert(
            "description".to_owned(),
            format!(
                "How long the store's lock holds before it is taken as abandoned, \"{}m\" by default and at most \"{}m\".",
                Self::DEFAULT.0.as_mins(),
                Self::MOST.0.as_mins()
            )
            .into(),
        );
        schema
    }
}

/// The times a lock with no time to judge its age by is found held,
/// unchanged, before it is taken as abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LockSightings {}

impl Counted for LockSightings {
    const NAME: &'static str = "LockPatience";
    const KEY: &'static str = "lock_patience";
    const VERB: &'static str = "waits for";
    const UNIT: &'static str = "sightings";
    const DESCRIPTION: &'static str = "How many times a lock with no time to judge its age by is found held, unchanged, before it is taken as abandoned";
    const DEFAULT: u8 = 10;
}

/// How many times an undated lock is found held before it is taken as
/// abandoned.
pub type LockPatience = Count<LockSightings>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_s_lease_is_10_minutes_by_default_and_at_most_40() {
        assert_eq!(LockLease::default().get(), SignedDuration::from_mins(10));
        assert_eq!(
            serde_json::from_str::<LockLease>(r#""40m""#)
                .map(LockLease::get)
                .ok(),
            Some(SignedDuration::from_mins(40))
        );
        assert_eq!(
            serde_json::from_str::<LockLease>(r#""41m""#)
                .unwrap_err()
                .to_string(),
            "lock_lease holds at most \"40m\", four times its default, not 41m"
        );
        let schema = serde_json::to_value(schemars::schema_for!(LockLease)).unwrap();
        assert_eq!(
            schema["description"],
            "How long the store's lock holds before it is taken as abandoned, \"10m\" by default and at most \"40m\"."
        );
    }

    #[test]
    fn a_lock_s_patience_is_10_sightings_by_default_and_at_most_40() {
        assert_eq!(LockPatience::default().get(), 10);
        assert_eq!(
            serde_json::from_str::<LockPatience>("41")
                .unwrap_err()
                .to_string(),
            "lock_patience waits for 1 to 40 sightings, not 41"
        );
    }
}
