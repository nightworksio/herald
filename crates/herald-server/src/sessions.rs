//! The sessions `herald server` holds (ADR-0016, ADR-0017): each command's
//! work joins the server's queue, with every other request's.

use herald_core::DestinationName;
use herald_flows::{Job, Taker};

use crate::{Heard, Held, Queue};

/// The server's queue, as the sessions hand it work, and where they say what
/// they came to.
pub(crate) struct Queued<'q> {
    pub(crate) queue: &'q Queue,
    pub(crate) heard: &'q dyn Fn(Heard),
}

impl<'q> Taker for Queued<'q> {
    type Place = Held<'q>;

    fn place(&self) -> Option<Held<'q>> {
        self.queue.hold()
    }

    fn fill(&self, place: Held<'q>, job: Job) {
        place.fill(job);
    }

    fn report(&self, destination: &DestinationName, said: String) {
        (self.heard)(Heard::Session {
            destination: destination.clone(),
            said,
        });
    }
}
