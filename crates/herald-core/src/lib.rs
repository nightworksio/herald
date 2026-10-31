//! herald's domain: the values it decides with and the steps between them.
//!
//! The crate performs no I/O. It reads no clock, no environment and no
//! randomness, and it is built for `wasm32-unknown-unknown` as well as native
//! targets.

mod chunk;
mod event;
mod housekeep;
mod key;
mod ledger;
mod markup;
mod profile;
mod render;
mod route;

pub use chunk::{Room, chunk, size};
pub use event::{Draft, Event, Kind, Problem};
pub use housekeep::{Chore, Schedule, due, schedules};
pub use key::{Identity, Key, NotAKey, RouteId};
pub use ledger::{Entry, Facts, Lease, Plan, Planned, Record, State, Step, plan, steps};
pub use markup::{Text, convert};
pub use profile::{Dialect, Features, Limits, Measure, MentionStyle, Profile, Threads};
pub use render::{Context, TemplateError, Templates};
pub use route::{
    Delivery, DestinationName, Filter, Mentions, Prereleases, RoleId, Route, SourceName,
    TemplateName, Thread, route,
};
