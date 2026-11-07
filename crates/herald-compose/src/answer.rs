use herald_flows::{Answered, Job, Place, Reply};

/// Work left to do, holding its place in the room until it is done.
#[derive(Debug)]
pub struct Left {
    pub(crate) job: Job,
    pub(crate) place: Place,
}

impl Left {
    /// `job`, holding `place`.
    #[must_use]
    pub fn new(job: Job, place: Place) -> Self {
        Self { job, place }
    }
}

/// What herald answers a request on a host: the reply, the reason a refusal
/// is logged with, and the work it leaves, holding its place.
#[derive(Debug)]
pub struct Answer {
    /// The reply.
    pub reply: Reply,
    /// Why the request was refused, for the log alone (ADR-0007).
    pub refused: Option<String>,
    /// The work to do once the reply is sent.
    pub left: Option<Left>,
}

impl From<Answered> for Answer {
    fn from(answered: Answered) -> Self {
        Self {
            reply: answered.reply,
            refused: answered.refused,
            left: None,
        }
    }
}
