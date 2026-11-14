use std::sync::atomic::{AtomicUsize, Ordering};

use jiff::{SignedDuration, Timestamp};

use crate::announce::Stop;

/// What a run may still spend before its host stops it: the time its work
/// may run until, and the outbound requests it may make. A send starts only
/// where both leave their spare.
#[derive(Debug, Clone, Copy)]
pub struct Budget<'a> {
    /// When the host stops the work, where it does.
    pub until: Option<Timestamp>,
    /// How much time must be left for a send to start.
    pub spare: SignedDuration,
    /// The outbound requests the run may make, where the host bounds them.
    pub requests: Option<Requests<'a>>,
}

/// The outbound requests a run may make: the count of those made, the most
/// the host allows, and how many must be left for a send to start.
#[derive(Debug, Clone, Copy)]
pub struct Requests<'a> {
    /// How many have been made.
    pub made: &'a AtomicUsize,
    /// How many had been made when the run's request arrived.
    pub from: usize,
    /// The most the host allows.
    pub most: usize,
    /// How many must be left for a send to start.
    pub spare: usize,
}

impl Budget<'_> {
    /// No bound at all.
    pub const UNBOUNDED: Budget<'static> = Budget {
        until: None,
        spare: SignedDuration::ZERO,
        requests: None,
    };

    /// Whether `sends` more sends may start at `now`, or else the stop that
    /// hands what is left to the next run.
    pub(crate) fn allows(&self, now: Timestamp, sends: usize) -> Result<(), Stop> {
        if let Some(until) = self.until
            && !now.checked_add(self.spare).is_ok_and(|ready| ready < until)
        {
            return Err(Stop::Limit(format!(
                "{:#} was left before the host stops the work, no more than the {:#} a send needs, so the next run sends the rest",
                until.duration_since(now).max(SignedDuration::ZERO),
                self.spare
            )));
        }
        if let Some(requests) = self.requests {
            let made = requests
                .made
                .load(Ordering::Relaxed)
                .saturating_sub(requests.from);
            if made
                .checked_add(sends)
                .and_then(|needed| needed.checked_add(requests.spare))
                .is_none_or(|needed| needed > requests.most)
            {
                return Err(Stop::Limit(format!(
                    "{made} of the {} outbound requests the host allows were made, so the next run sends the rest",
                    requests.most
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp::constant(1_700_000_000, 0);

    #[test]
    fn a_send_starts_only_while_the_time_left_holds_the_spare() {
        let budget = |left| Budget {
            until: Some(NOW.checked_add(SignedDuration::from_secs(left)).unwrap()),
            spare: SignedDuration::from_secs(10),
            requests: None,
        };
        assert_eq!(budget(11).allows(NOW, 1), Ok(()));
        assert!(
            matches!(budget(10).allows(NOW, 1), Err(Stop::Limit(said)) if said.starts_with("10s was left before the host stops the work, no more than the 10s"))
        );
        assert_eq!(
            budget(-5).allows(NOW, 1),
            Err(Stop::Limit(
                "0s was left before the host stops the work, no more than the 10s a send needs, so the next run sends the rest"
                    .to_owned()
            ))
        );
        assert_eq!(Budget::UNBOUNDED.allows(NOW, usize::MAX), Ok(()));
        let far = Budget {
            until: Some(Timestamp::MAX),
            spare: SignedDuration::MAX,
            requests: None,
        };
        assert!(far.allows(NOW, 1).is_err());
    }

    #[test]
    fn a_send_starts_only_while_the_requests_left_hold_it_and_the_spare() {
        let made = AtomicUsize::new(30);
        let budget = |most| Budget {
            requests: Some(Requests {
                made: &made,
                from: 3,
                most,
                spare: 4,
            }),
            ..Budget::UNBOUNDED
        };
        assert_eq!(budget(33).allows(NOW, 2), Ok(()));
        assert_eq!(
            budget(32).allows(NOW, 2),
            Err(Stop::Limit(
                "27 of the 32 outbound requests the host allows were made, so the next run sends the rest"
                    .to_owned()
            ))
        );
        assert_eq!(budget(32).allows(NOW, 1), Ok(()));
        assert!(budget(usize::MAX).allows(NOW, usize::MAX).is_err());
    }
}
