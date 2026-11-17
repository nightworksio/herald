//! What each project last announced, kept as a route announces it
//! (ADR-0016).

use herald_core::{Event, Latest, RouteId};
use herald_ports::{Announcements, Store, Written};

use super::{Outcome, Stop, stored};

/// `outcome`, once `event` is kept as what its project last announced on
/// `route`, where it was sent or had been and the store keeps that, or
/// stopped by why it could not be kept (ADR-0016). An announcement already
/// complete keeps it again, so a run that could not keep it is made good by
/// the next.
pub(crate) async fn kept_latest<S: Store>(
    store: &S,
    event: &Event,
    route: &RouteId,
    outcome: Outcome,
    attempts: usize,
) -> Outcome {
    let landed = match &outcome {
        Outcome::Sent { steps } => steps.clone(),
        Outcome::AlreadyAnnounced => Vec::new(),
        Outcome::Held { .. }
        | Outcome::Gathered { .. }
        | Outcome::Stopped { .. }
        | Outcome::Unedited
        | Outcome::Skipped(_) => return outcome,
    };
    let Some(announcements) = store.announcements() else {
        return outcome;
    };
    match keep_latest(announcements, event, route, attempts).await {
        Ok(()) => outcome,
        Err(reason) => Outcome::Stopped {
            landed,
            remaining: Vec::new(),
            reason,
        },
    }
}

async fn keep_latest<A: Announcements>(
    announcements: &A,
    event: &Event,
    route: &RouteId,
    attempts: usize,
) -> Result<(), Stop> {
    for _ in 0..attempts {
        let kept = announcements
            .latest(event.project())
            .await
            .map_err(stored)?;
        let expected = kept.as_ref().map(|kept| kept.version);
        let latest = Latest::after(kept.map(|kept| kept.value), event, route);
        if announcements
            .put_latest(&latest, expected)
            .await
            .map_err(stored)?
            != Written::Conflict
        {
            return Ok(());
        }
    }
    Err(Stop::Store(format!(
        "what {} last announced changed under each of {attempts} writes",
        event.project()
    )))
}
