//! herald's domain: the values it decides with and the steps between them.
//!
//! The crate performs no I/O. It reads no clock, no environment and no
//! randomness, and it is built for `wasm32-unknown-unknown` as well as native
//! targets.

mod activity;
mod address;
mod announce;
mod bounds;
mod bump;
mod chunk;
mod credentials;
mod cursor;
mod declined;
pub mod der;
mod duration;
mod escaping;
mod event;
pub mod follows;
mod glob;
mod group;
mod housekeep;
mod inbound;
mod installation;
mod interact;
pub mod itself;
mod kept;
mod key;
mod kind;
mod latest;
mod ledger;
mod marker;
mod markup;
mod matching;
pub mod media;
mod members;
mod origin;
mod pattern;
mod pending;
mod profile;
mod published;
mod queue;
mod rate;
mod render;
pub mod report;
mod retention;
mod revision;
mod route;
mod signal;
mod size;
pub mod status;
mod subscription;
mod symbols;
mod token;
mod version;
mod waits;
mod withdrawal;
mod wording;

pub use activity::{ACTIVITY_JSON, SIGNED, Signing, public_pem, signature_header, signing};
pub use address::{Address, domain, path_of_names, socket_under, under_domain, web_url};
pub use announce::{
    Announcement, Button, Title, announce, body_template, group_template, thread_template,
    title_template,
};
pub use bounds::{
    Attempts, Count, Counted, Inbound, LookedUpPins, PinLookups, ReadBack, ReadBackPages, Sandbox,
    WriteAttempts,
};
pub use bump::{Bump, BumpPattern, Bumper, NotABumpPattern, bumped, read_bump};
pub use chunk::{Room, chunk, size};
pub use credentials::{basic, bot_or_webhook};
pub use cursor::{Cursor, Polled, Proof, Tombstone, Unheld, fresh};
pub use declined::Declined;
pub use duration::Duration;
pub use escaping::{Escaping, safe_url, unreserved};
pub use event::{Draft, Event, Problem};
pub use glob::{Glob, NotAGlob};
pub use group::{
    Change, GROUP_KEY_MOST, GROUP_MOST, GROUP_MOST_ALLOWED, GROUP_WINDOW, Group, GroupLate,
    GroupMode, GroupUntil, Grouping, Subject, group_identity, shown,
};
pub use housekeep::{Chore, Schedule, due, schedules};
pub use inbound::{
    Addressed, Algorithm, Claim, KeyForm, Refusal, Scheme, Signature, Signed, activity_claim,
    bearer, claim, key, same,
};
pub use installation::Installation;
pub use interact::{
    AnnouncedHere, Answering, Asked, Command, Member, Place, Replies, Said, Unchosen, announced,
    chosen, completions, every_command,
};
pub use kept::RecordKind;
pub use key::{Identity, Key, NotAKey, RouteId, SHA256_HEX_DIGITS, digest, hex, unhex};
pub use kind::{
    ACTION, ACTOR, ALERT_LINK, ATTEMPT, AUTHOR, Attribute, AttributeSpec, AttributeType, BASE, BOT,
    BRANCH, CHANGELOG_LINK, COMMIT, CONCLUSION, DRAFT, ECOSYSTEM, ENTRY_LINK, FROM, HEAD,
    ISSUE_LINK, Kind, KindSpec, KindSpecs, LABELS, LinkSpec, MERGED_BY, NUMBER, PACKAGE,
    PACKAGE_TYPE, PATH, PRERELEASE, PULL_LINK, PULLS, RELEASE_LINK, RESOLUTION, RULE, RUN_LINK, SEVERITY,
    STALE, STATE_REASON, TOOL, TRIGGER, VERSION, VIEW, WORKFLOW, readable, versioned,
};
pub use latest::{Latest, Shown};
pub use ledger::{
    Entry, Facts, Lease, Plan, Planned, Record, Sent, Standing, State, Step, plan, steps,
};
pub use marker::{Marker, Marks};
pub use markup::{LINK_SCHEMES, Text, convert, escaped, literal};
pub use matching::Match;
pub use members::{
    Delivered, EventsRead, KEEP_MEMBERS, Listed, Listing, MOST_LISTED, MOST_MEMBERS,
    MOST_MEMBERS_ALLOWED, Membership, Owner, Selection, Visibility,
};
pub use origin::{Origin, kind_name};
pub use pattern::Pattern;
pub use pending::{HeldEvent, Hold, LONGEST_HOLD, Pending};
pub use profile::{
    Bullet, Emphases, Escape, Features, Fences, Heading, Limits, Links, Markup, Measure,
    MentionRoom, MentionStyle, Pins, Profile, Threads,
};
pub use published::{Feed, FeedItem, PeerKeys, PublishedKeys};
pub use queue::{
    Batch, Board, Bots, Checks, Deciding, GroupBy, Grouped, Mergeable, Pull, Reason, Review, Row,
    Rules, Showing, Turn, View, board,
};
pub use rate::{Per, Rate};
pub use render::{
    ACTOR_FIELDS, ActorName, Context, Form, ProjectName, REPOSITORY_FIELDS, TemplateError,
    Templates, view_json, waited,
};
pub use report::{REPORT_PREFIX, ReportSchedule, report_event, report_identity, zone};
pub use retention::Retention;
pub use revision::{
    Edits, LateRelease, NO_PART, Revise, Revision, Withdraw, already_deleted, regrouped, revise,
    revised,
};
pub use route::{
    Delivery, DestinationName, Filter, Mentions, Pinning, Prereleases, Reach, RoleId, Route,
    SecretRef, SourceName, TemplateName, Thread, route,
};
pub use signal::{EDITED, Prefer, PrereleaseRule, SIGNAL, Signal, Validators, WITHDRAWN};
pub use size::Size;
pub use subscription::Subscription;
pub use symbols::Symbols;
pub use token::KeptToken;
pub use version::prerelease_version;
pub use waits::{WaitOptions, Waits};
pub use withdrawal::{reissued, withdrawal};
pub use wording::{Phrase, Wording};
