use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::duration::Duration;
use crate::status;

/// How long a destination waits before it sends again, where its platform
/// answers without naming a wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waits {
    /// After the platform answers that it is unwell.
    pub outage: SignedDuration,
    /// After the platform answers that herald sends too often.
    pub limited: SignedDuration,
}

/// The waits as `[destinations.*] waits` writes them, each a length of time
/// of at least a second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitOptions {
    /// After the platform answers that it is unwell; "5s" where unset.
    pub outage: Option<Duration>,
    /// After the platform answers that herald sends too often; the
    /// platform's own window where unset: "5m" on Mastodon, "5s" elsewhere.
    pub limited: Option<Duration>,
}

impl Waits {
    /// The shortest wait a config may set.
    pub(crate) const SHORTEST: SignedDuration = SignedDuration::from_secs(1);
    /// The waits where neither the config nor the platform sets any.
    pub const DEFAULT: Self = Self {
        outage: SignedDuration::from_secs(5),
        limited: SignedDuration::from_secs(5),
    };

    /// The waits `set` gives, `platform`'s own where it leaves one out.
    #[must_use]
    pub fn of(set: Option<WaitOptions>, platform: Self) -> Self {
        let set = set.unwrap_or_default();
        Self {
            outage: set.outage.map_or(platform.outage, |wait| wait.0),
            limited: set.limited.map_or(platform.limited, |wait| wait.0),
        }
    }

    /// How long to wait after an answer of `answered`: the wait the answer
    /// `named`, else this one; none where `answered` asks for no wait.
    #[must_use]
    pub const fn after(
        self,
        answered: u16,
        named: Option<SignedDuration>,
    ) -> Option<SignedDuration> {
        let unnamed = match answered {
            status::TOO_MANY_REQUESTS => self.limited,
            status::SERVER_ERROR.. => self.outage,
            _ => return None,
        };
        match named {
            Some(wait) => Some(wait),
            None => Some(unnamed),
        }
    }

    /// Whether every wait `set` gives is long enough.
    ///
    /// # Errors
    ///
    /// The first that is not, naming it.
    pub fn check(set: &WaitOptions) -> Result<(), String> {
        for (name, wait) in [("outage", set.outage), ("limited", set.limited)] {
            if let Some(wait) = wait
                && wait.0 < Self::SHORTEST
            {
                return Err(format!(
                    "waits.{name} of {:#} would ask again at once; wait at least {:#}",
                    wait.0,
                    Self::SHORTEST
                ));
            }
        }
        Ok(())
    }
}

impl Default for Waits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTODON: Waits = Waits {
        limited: SignedDuration::from_mins(5),
        ..Waits::DEFAULT
    };

    fn set(outage: Option<i64>, limited: Option<i64>) -> WaitOptions {
        let seconds = |secs| Duration(SignedDuration::from_secs(secs));
        WaitOptions {
            outage: outage.map(seconds),
            limited: limited.map(seconds),
        }
    }

    #[test]
    fn a_wait_the_config_leaves_out_is_the_platform_s_own() {
        assert_eq!(Waits::of(None, MASTODON), MASTODON);
        assert_eq!(Waits::default(), Waits::DEFAULT);
        assert_eq!(
            Waits::of(Some(set(Some(30), None)), MASTODON),
            Waits {
                outage: SignedDuration::from_secs(30),
                limited: SignedDuration::from_mins(5),
            }
        );
        assert_eq!(
            Waits::of(Some(set(None, Some(60))), Waits::DEFAULT),
            Waits {
                outage: SignedDuration::from_secs(5),
                limited: SignedDuration::from_secs(60),
            }
        );
    }

    #[test]
    fn a_limit_waits_limited_and_an_outage_outage_unless_the_answer_names_a_wait() {
        let named = Some(SignedDuration::from_secs(42));
        assert_eq!(
            MASTODON.after(429, None),
            Some(SignedDuration::from_mins(5))
        );
        assert_eq!(MASTODON.after(429, named), named);
        assert_eq!(
            MASTODON.after(500, None),
            Some(SignedDuration::from_secs(5))
        );
        assert_eq!(MASTODON.after(503, named), named);
        assert_eq!(
            MASTODON.after(599, None),
            Some(SignedDuration::from_secs(5))
        );
        for quiet in [200, 404, 428, 430, 499] {
            assert_eq!(MASTODON.after(quiet, named), None, "{quiet}");
        }
    }

    #[test]
    fn a_wait_is_at_least_a_second() {
        assert_eq!(Waits::check(&set(Some(1), Some(1))), Ok(()));
        assert_eq!(Waits::check(&WaitOptions::default()), Ok(()));
        let short = WaitOptions {
            outage: None,
            limited: Some(Duration(SignedDuration::from_millis(999))),
        };
        let refused = Waits::check(&short).unwrap_err();
        assert!(refused.starts_with("waits.limited of 999ms"), "{refused}");
        assert!(refused.ends_with("wait at least 1s"), "{refused}");
        let refused = Waits::check(&set(Some(0), None)).unwrap_err();
        assert!(refused.starts_with("waits.outage of 0s"), "{refused}");
    }
}
