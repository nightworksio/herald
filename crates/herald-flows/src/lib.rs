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
mod commanded;
mod doctor;
mod following;
mod housekeep;
mod inbound;
mod interact;
mod interacting;
mod poll;
mod prepare;
mod preview;
mod reached;
mod room;
mod round;
mod serving;
mod sessions;
mod together;

pub use announce::{Announced, Outcome, Ports, Stop, announce, announce_within};
pub use approving::{Decision, decide, followers};
pub use commanded::{INTERACTIONS, MATRIX};
pub use doctor::{Findings, Gap, Layer, doctor, keys, unserved};
pub use following::{FollowError, Followed, followed};
pub use housekeep::{Chored, Housekept, Kept, housekeep};
pub use inbound::{
    Admission, Checking, Keys, Rejected, Verified, activity_verified, admitted, released, verified,
};
pub use interact::{Answer, completed, deferral, interact, replied};
pub use interacting::{Interacting, Replying};
pub use poll::{Fresh, Remembered, Sweep, Swept, poll};
pub use prepare::{Connected, PrepareError, Prepared, prepare};
pub use preview::{Previewed, preview};
pub use reached::{Posts, Reached};
pub use room::{Place, Room, Running};
pub use round::{ROUND_SECRET, Round};
pub use serving::{
    Answered, HEALTH, HOOKS, Inbound, Interacted, Job, Reply, Serving, Webhooks, Worked, answer,
    busy, refusal, uncomposed, unworked, webhooks, work,
};
pub use sessions::{Sessions, Taker};
