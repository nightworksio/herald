use herald_core::{DELIVERIES_KEPT, Schedule, Sent, due};
use herald_ports::{Clock, Http, Store};

use crate::announce::Stop;
use crate::prepare::Connected;

/// What housekeeping did with one schedule that fell due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    /// The schedule.
    pub schedule: Schedule,
    /// What became of it.
    pub outcome: Chored,
}

/// What became of a schedule that fell due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chored {
    /// The chore was done, and the schedule removed.
    Done,
    /// The chore cannot be done, for this reason, and the schedule was
    /// removed.
    Dropped(String),
    /// The chore was not done now, for this reason, and the schedule stays.
    Later(Stop),
}

/// Every chore due at the clock's time done through the route that
/// scheduled it, each schedule removed once its chore is done or cannot be
/// (ADR-0005), after every delivery the store admitted more than
/// [`DELIVERIES_KEPT`] ago is forgotten (ADR-0013).
///
/// # Errors
///
/// When the store's schedules cannot be read, or its deliveries cannot be
/// expired.
pub async fn housekeep<S: Store, C: Clock, H: Http>(
    connected: &Connected<H>,
    store: &S,
    clock: &C,
) -> Result<Vec<Kept>, Stop> {
    if store.capabilities().deliveries {
        store
            .expire_deliveries(clock.now() - DELIVERIES_KEPT)
            .await
            .map_err(|error| Stop::Store(error.to_string()))?;
    }
    let schedules = store
        .schedules()
        .await
        .map_err(|error| Stop::Store(error.to_string()))?;
    let mut kept = Vec::new();
    let mut waiting: Option<Stop> = None;
    for schedule in due(&schedules, clock.now()) {
        if let Some(stop) = &waiting {
            kept.push(Kept {
                schedule: schedule.clone(),
                outcome: Chored::Later(stop.clone()),
            });
            continue;
        }
        let route = connected
            .routes
            .iter()
            .find(|(route, _)| route.config.route.id == schedule.route);
        let outcome = match route {
            None => Chored::Dropped("the route is no longer in the config".to_owned()),
            Some((route, http)) => {
                match route
                    .reached
                    .housekeep(http, schedule.chore, &schedule.thread)
                    .await
                {
                    Ok(Sent::Landed(_)) => Chored::Done,
                    Ok(Sent::Refused(reason)) => Chored::Dropped(reason),
                    Ok(Sent::RetryAfter(after)) => {
                        waiting = Some(Stop::RetryAfter(after));
                        Chored::Later(Stop::RetryAfter(after))
                    }
                    Err(error) => Chored::Later(Stop::Unreachable(error.to_string())),
                }
            }
        };
        let outcome = match outcome {
            Chored::Later(stop) => Chored::Later(stop),
            settled => match store
                .put_schedules(&[], std::slice::from_ref(schedule))
                .await
            {
                Ok(()) => settled,
                Err(error) => Chored::Later(Stop::Store(error.to_string())),
            },
        };
        kept.push(Kept {
            schedule: schedule.clone(),
            outcome,
        });
    }
    Ok(kept)
}
