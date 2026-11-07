//! herald's domain: the values it decides with and the steps between them.
//!
//! The crate performs no I/O. It reads no clock, no environment and no
//! randomness, and it is built for `wasm32-unknown-unknown` as well as native
//! targets.

mod activity;
mod announce;
mod chunk;
mod cursor;
mod event;
mod housekeep;
mod inbound;
mod interact;
mod key;
mod latest;
mod ledger;
mod marker;
mod markup;
mod pattern;
mod profile;
mod render;
mod route;
mod subscription;
mod version;

pub use activity::{
    ACTIVITY_JSON, SIGNED, Signing, body_sha256, public_pem, signature_header, signing,
};
pub use announce::{
    Announcement, Title, announce, body_template, shortened, thread_template, title_template,
};
pub use chunk::{Room, chunk, size};
pub use cursor::{Cursor, Polled, fresh};
pub use event::{Draft, Event, Kind, Problem};
pub use housekeep::{Chore, Schedule, due, schedules};
pub use inbound::{
    Addressed, Claim, DELIVERIES_KEPT, MOST_COVERED, MOST_SIGNATURES, REPLAY_WINDOW, Refusal,
    Scheme, Signature, activity_claim, claim, key, same,
};
pub use interact::{
    AnnouncedHere, Answering, Asked, Command, MOST_COMPLETIONS, Member, Place, Replies, Said,
    Unchosen, announced, chosen, completions, every_command,
};
pub use key::{Identity, Key, NotAKey, RouteId, digest, hex};
pub use latest::{Latest, Shown};
pub use ledger::{
    Entry, Facts, Lease, Plan, Planned, Record, Sent, Standing, State, Step, plan, steps,
};
pub use marker::{Marker, Marks};
pub use markup::{LINK_SCHEMES, Text, convert, escaped, literal, safe_url};
pub use pattern::Pattern;
pub use profile::{Dialect, Features, Limits, Measure, MentionStyle, Pins, Profile, Threads};
pub use render::{Context, Form, TemplateError, Templates};
pub use route::{
    Delivery, DestinationName, Filter, Mentions, Pinning, Prereleases, RoleId, Route, SecretRef,
    SourceName, TemplateName, Thread, route,
};
pub use subscription::Subscription;
pub use version::prerelease_version;
