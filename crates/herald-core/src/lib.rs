//! herald's domain: the values it decides with and the steps between them.
//!
//! The crate performs no I/O. It reads no clock, no environment and no
//! randomness, and it is built for `wasm32-unknown-unknown` as well as native
//! targets.

mod activity;
mod address;
mod announce;
mod bounds;
mod chunk;
mod credentials;
mod cursor;
mod declined;
pub mod der;
mod duration;
mod escaping;
mod event;
mod housekeep;
mod inbound;
mod interact;
pub mod itself;
mod kept;
mod key;
mod kind;
mod latest;
mod ledger;
mod marker;
mod markup;
pub mod media;
mod pattern;
mod profile;
mod render;
mod retention;
mod route;
mod size;
pub mod status;
mod subscription;
mod symbols;
mod version;
mod waits;
mod wording;

pub use activity::{ACTIVITY_JSON, SIGNED, Signing, public_pem, signature_header, signing};
pub use address::{Address, domain, path_of_names, socket_under, under_domain, web_url};
pub use announce::{
    Announcement, Button, Title, announce, body_template, thread_template, title_template,
};
pub use bounds::{
    Attempts, Count, Counted, Inbound, LookedUpPins, PinLookups, ReadBack, ReadBackPages, Sandbox,
    WriteAttempts,
};
pub use chunk::{Room, chunk, size};
pub use credentials::{basic, bot_or_webhook};
pub use cursor::{Cursor, Polled, fresh};
pub use declined::Declined;
pub use duration::Duration;
pub use escaping::{Escaping, safe_url, unreserved};
pub use event::{Draft, Event, Problem};
pub use housekeep::{Chore, Schedule, due, schedules};
pub use inbound::{
    Addressed, Algorithm, Claim, KeyForm, Refusal, Scheme, Signature, Signed, activity_claim,
    bearer, claim, key, same,
};
pub use interact::{
    AnnouncedHere, Answering, Asked, Command, Member, Place, Replies, Said, Unchosen, announced,
    chosen, completions, every_command,
};
pub use kept::RecordKind;
pub use key::{Identity, Key, NotAKey, RouteId, SHA256_HEX_DIGITS, digest, hex, unhex};
pub use kind::{
    Attribute, AttributeSpec, AttributeType, CHANGELOG_LINK, ENTRY_LINK, Kind, KindSpec, KindSpecs,
    LinkSpec, PRERELEASE, RELEASE_LINK, VERSION, readable, versioned,
};
pub use latest::{Latest, Shown};
pub use ledger::{
    Entry, Facts, Lease, Plan, Planned, Record, Sent, Standing, State, Step, plan, steps,
};
pub use marker::{Marker, Marks};
pub use markup::{LINK_SCHEMES, Text, convert, escaped, literal};
pub use pattern::Pattern;
pub use profile::{
    Bullet, Emphases, Escape, Features, Fences, Heading, Limits, Links, Markup, Measure,
    MentionRoom, MentionStyle, Pins, Profile, Threads,
};
pub use render::{Context, Form, TemplateError, Templates};
pub use retention::Retention;
pub use route::{
    Delivery, DestinationName, Filter, Mentions, Pinning, Prereleases, RoleId, Route, SecretRef,
    SourceName, TemplateName, Thread, route,
};
pub use size::Size;
pub use subscription::Subscription;
pub use symbols::Symbols;
pub use version::prerelease_version;
pub use waits::{WaitOptions, Waits};
pub use wording::{Phrase, Wording};
