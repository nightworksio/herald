//! herald's commands as flows over the ports (ADR-0001): each driver
//! constructs its ports and hands them to the same flow.
//!
//! [`prepare`] resolves a config's credentials and marker keys, and
//! [`Prepared::connect`] gives each route and source the `Http` that
//! reaches it. [`announce`] then sends an event to every route that takes
//! it, once, finishing what an earlier run left (ADR-0005); [`poll`] asks
//! each source for what it has not seen and announces it (ADR-0003); and
//! [`housekeep`] does each thread's chores as they fall due. [`doctor`]
//! names what a config asks for that cannot be served, and [`preview`]
//! renders each route's templates as its destination would show them.

mod announce;
mod doctor;
mod housekeep;
mod poll;
mod prepare;
mod preview;
mod reached;

pub use announce::{Announced, Outcome, Ports, Stop, announce};
pub use doctor::{Findings, Gap, doctor, unserved};
pub use housekeep::{Chored, Kept, housekeep};
pub use poll::{Fresh, Remembered, Sweep, Swept, poll};
pub use prepare::{Connected, PrepareError, Prepared, prepare};
pub use preview::{Previewed, preview};
pub use reached::{Posts, Reached};
