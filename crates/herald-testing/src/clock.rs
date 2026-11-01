use std::sync::Mutex;

use herald_ports::Clock;

use crate::lock;
use jiff::{SignedDuration, Timestamp};

/// A clock that says what it is told.
#[derive(Debug)]
pub struct FakeClock {
    now: Mutex<Timestamp>,
}

impl FakeClock {
    /// A clock at `now`.
    #[must_use]
    pub fn at(now: Timestamp) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    /// Moves the clock on by `by`.
    pub fn advance(&self, by: SignedDuration) {
        let mut now = lock(&self.now);
        *now = now.checked_add(by).unwrap_or(Timestamp::MAX);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *lock(&self.now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_says_what_it_is_told_and_moves_when_advanced() {
        let clock = FakeClock::at("2026-10-31T12:00:00Z".parse().unwrap());
        assert_eq!(clock.now().to_string(), "2026-10-31T12:00:00Z");
        clock.advance(SignedDuration::from_mins(5));
        assert_eq!(clock.now().to_string(), "2026-10-31T12:05:00Z");
        clock.advance(SignedDuration::MAX);
        assert_eq!(clock.now(), Timestamp::MAX);
    }
}
