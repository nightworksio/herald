use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use herald_kinds::Unpolled;
use herald_ports::{Clock, Http, HttpError, Request, Response};
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

    /// It, leaving `kept` more of its requests unspent for what follows.
    #[must_use]
    pub(crate) fn keeping(self, kept: usize) -> Self {
        Self {
            requests: self.requests.map(|requests| Requests {
                spare: requests.spare.saturating_add(kept),
                ..requests
            }),
            ..self
        }
    }

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

impl<S, C, G> crate::announce::Ports<'_, S, C, G> {
    /// The ports, their budget leaving `kept` more of its requests unspent
    /// for what follows.
    pub(crate) fn keeping(&self, kept: usize) -> Self {
        Self {
            store: self.store,
            clock: self.clock,
            signer: self.signer,
            budget: self.budget.keeping(kept),
        }
    }
}

/// Why a release's header left a repository unread where the run's budget
/// ended.
pub(crate) const HEADER_BUDGET_ENDED: &str =
    "the run's request budget ended before its milestones were read; the next run reads them";

/// Why an overview's section was left unread where the run's budget ended.
pub(crate) const BUDGET_ENDED: &str =
    "the run's request budget ended before its runs were read; the next run reads them";

/// Why an advisories section was left unread where the run's budget ended.
pub(crate) const ADVISORIES_BUDGET_ENDED: &str =
    "the run's request budget ended before its advisories were read; the next run reads them";

/// An `Http` that starts each send only where a run's budget allows one
/// more: the one home every read of a forge a run makes goes through,
/// however many requests it takes (ADR-0062).
pub(crate) struct Budgeted<'a, H, C> {
    http: &'a H,
    budget: Budget<'a>,
    clock: &'a C,
    refused: Mutex<Option<Stop>>,
}

impl<'a, H: Http, C: Clock> Budgeted<'a, H, C> {
    /// `http`, each send starting only where `budget` allows one at the
    /// time `clock` tells.
    pub(crate) const fn new(http: &'a H, budget: Budget<'a>, clock: &'a C) -> Self {
        Self {
            http,
            budget,
            clock,
            refused: Mutex::new(None),
        }
    }

    /// Why a read of an overview that failed as `unpolled` left its
    /// section unread, in plain words: the run's budget ending leaves the
    /// section unread, never the whole report stopped (ADR-0066).
    pub(crate) fn unread(&self, unpolled: Unpolled) -> String {
        self.left_unread(unpolled, (BUDGET_ENDED, "runs"))
    }

    /// Why a read of advisories that failed as `unpolled` left its section
    /// unread, in the words [`Self::unread`] gives a capacity read
    /// (ADR-0067).
    pub(crate) fn unread_advisories(&self, unpolled: Unpolled) -> String {
        self.left_unread(unpolled, (ADVISORIES_BUDGET_ENDED, "advisories"))
    }

    /// Why a read of `what` that failed as `unpolled` left it unread:
    /// `ended` where the run's budget refused a send.
    fn left_unread(&self, unpolled: Unpolled, (ended, what): (&str, &str)) -> String {
        let refused = self
            .refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match (refused, unpolled) {
            (Some(_), _) => ended.to_owned(),
            (None, Unpolled::Unreachable(reason)) => {
                format!("its forge could not be reached: {reason}")
            }
            (None, Unpolled::Unreadable(reason) | Unpolled::RateLimited(reason)) => {
                format!("its {what} could not be read: {reason}")
            }
        }
    }

    /// Why a read of a release's milestones that failed as `unpolled` left
    /// its repository unread, in the words [`Self::unread`] gives a
    /// capacity read (ADR-0082).
    pub(crate) fn unread_milestones(&self, unpolled: Unpolled) -> String {
        self.left_unread(unpolled, (HEADER_BUDGET_ENDED, "milestones"))
    }

    /// Whether a send was refused unsent, as the run's budget ended.
    pub(crate) fn unsent(&self) -> bool {
        self.refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// The stop a read that failed as `unpolled` ends with: the budget's,
    /// where it refused a send, and else what the forge's answer says.
    pub(crate) fn halted(&self, unpolled: Unpolled) -> Stop {
        self.refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or_else(|| crate::poll::halted(unpolled))
    }
}

/// An `Http` that names the stop a read through it ends with.
pub(crate) trait Halting: Http {
    /// The stop a read that failed as `unpolled` ends with.
    fn halted(&self, unpolled: Unpolled) -> Stop;
}

impl<H: Http, C: Clock> Halting for Budgeted<'_, H, C> {
    fn halted(&self, unpolled: Unpolled) -> Stop {
        Budgeted::halted(self, unpolled)
    }
}

impl<H: Http, C: Clock> Http for Budgeted<'_, H, C> {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        if let Err(stop) = self.budget.allows(self.clock.now(), 1) {
            let reason = format!("{stop:?}");
            *self.refused.lock().unwrap_or_else(PoisonError::into_inner) = Some(stop);
            return Err(HttpError::Unreachable { reason });
        }
        self.http.send(request).await
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
