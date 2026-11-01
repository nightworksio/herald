//! herald's commands as flows over the ports (ADR-0001): each driver
//! constructs its ports and hands them to the same flow.
//!
//! [`prepare`] resolves a config's credentials and marker keys, and
//! [`Prepared::connect`] gives each route the `Http` its destination may
//! reach. [`announce`] then sends an event to every route that takes it,
//! once, finishing what an earlier run left (ADR-0005), and [`housekeep`]
//! does each thread's chores as they fall due.

mod announce;
mod housekeep;
mod prepare;
mod reached;

pub use announce::{Announced, Outcome, Ports, Stop, announce};
pub use housekeep::{Chored, Kept, housekeep};
pub use prepare::{Connected, PrepareError, Prepared, prepare};
pub use reached::Reached;
