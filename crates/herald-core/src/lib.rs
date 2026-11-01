//! herald's domain: the values it decides with and the steps between them.
//!
//! The crate performs no I/O. It reads no clock, no environment and no
//! randomness, and it is built for `wasm32-unknown-unknown` as well as native
//! targets.

mod announce;
mod chunk;
mod cursor;
mod event;
mod housekeep;
mod key;
mod ledger;
mod marker;
mod markup;
mod profile;
mod render;
mod route;
mod subscription;
mod version;

pub use announce::{
    Announcement, announce, body_template, shortened, thread_template, title_template,
};
pub use chunk::{Room, chunk, size};
pub use cursor::{Cursor, fresh};
pub use event::{Draft, Event, Kind, Problem};
pub use housekeep::{Chore, Schedule, due, schedules};
pub use key::{Identity, Key, NotAKey, RouteId, hex};
pub use ledger::{Entry, Facts, Lease, Plan, Planned, Record, Sent, State, Step, plan, steps};
pub use marker::{Marker, Marks};
pub use markup::{Text, convert, literal};
pub use profile::{Dialect, Features, Limits, Measure, MentionStyle, Pins, Profile, Threads};
pub use render::{Context, Form, TemplateError, Templates};
pub use route::{
    Delivery, DestinationName, Filter, Mentions, Pinning, Prereleases, RoleId, Route, SecretRef,
    SourceName, TemplateName, Thread, route,
};
pub use subscription::Subscription;
pub use version::prerelease_version;
