//! herald's commands as flows over the ports (ADR-0001): each driver
//! constructs its ports and hands them to the same flow.
//!
//! [`prepare`] resolves a config's credentials and marker keys, and
//! [`Prepared::connect`] gives each route and source the `Http` that
//! reaches it. [`announce`] then sends an event to every route that takes
//! it, once, finishing what an earlier run left (ADR-0005); [`poll`] asks
//! each source for what it has not seen and announces it (ADR-0003); and
//! [`housekeep`] does each thread's chores as they fall due. [`verified`]
//! lets an inbound request through only once its signature is its sender's
//! (ADR-0007), and [`answer`] answers every request `herald server` and the
//! serverless drivers take, leaving its work to [`work`]. [`Sessions`]
//! holds the sessions a destination carries commands over. [`doctor`]
//! names what a config asks for that cannot be served, and [`preview`]
//! renders each route's templates as its destination would show them.

mod announce;
mod approving;
mod command_answers;
mod command_endpoints;
mod command_requests;
mod doctor;
mod follows;
mod housekeep;
mod poll;
mod prepare;
mod preview;
mod room;
mod round;
mod serving;
mod sessions;
mod trial;
mod verification;

pub use announce::{Announced, Outcome, Ports, Stop, announce, announce_within};
pub use approving::{Decision, decide, followers};
pub use command_answers::{Answer, completed, deferral, interact, replied};
pub use doctor::{
    Cadence, EGRESS_GUARDED, Egress, Findings, Gap, Here, Layer, Memory, Rounds, Tier, doctor,
    egress_guarded, keys, online, secrets, unserved,
};
pub use follows::{FollowError, Followed, followed};
pub use herald_kinds::Replying;
pub use housekeep::{Chored, Housekept, HousekeptSchedule, housekeep};
pub use poll::{Fresh, Remembered, Sweep, Swept, poll};
pub use prepare::{Connected, PrepareError, Prepared, UnreadSecret, prepare};
pub use preview::{Previewed, preview};
pub use room::{Place, Room, Running};
pub use round::{ROUND_SECRET, Round};
pub use serving::{
    Answered, Did, HEALTH, Inbound, Interacted, Job, Reply, Serving, Webhooks, Worked, answer,
    busy, refusal, uncomposed, unplaced, unworked, work,
};
pub use sessions::{Sessions, Taker};
pub use trial::trial;
pub use verification::{
    Admission, Checking, Keys, Rejected, Verified, activity_verified, admitted, released, verified,
};
