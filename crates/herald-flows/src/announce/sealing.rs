//! What a run tells of a route a view's advisories section is or was sent
//! to: sent or stopped, and nothing more (ADR-0067).

use super::Outcome;

/// `outcome` as a route a view's advisories section was sent to tells it:
/// sent or stopped, with nothing of its steps or of why it stopped, so no
/// destination's answer or wait tells what the section held (ADR-0067).
pub(crate) fn sealed(outcome: Outcome) -> Outcome {
    let stopped = match outcome {
        Outcome::Sent { .. } | Outcome::AlreadyAnnounced => false,
        Outcome::Sealed { stopped } => stopped,
        Outcome::Stopped { .. } => true,
        Outcome::Unedited
        | Outcome::Skipped(_)
        | Outcome::Gathered { .. }
        | Outcome::Held { .. } => return outcome,
    };
    Outcome::Sealed { stopped }
}

#[cfg(test)]
mod tests;
