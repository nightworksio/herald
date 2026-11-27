//! herald's domain: the values it decides with and the steps between them.
//!
//! The crate performs no I/O. It reads no clock, no environment and no
//! randomness, and it is built for `wasm32-unknown-unknown` as well as native
//! targets.

mod activity;
mod address;
mod announce;
mod audit;
mod bounds;
mod bump;
mod chunk;
mod claim_pattern;
mod credentials;
mod cursor;
mod decide;
mod declined;
mod dequeue;
pub mod der;
mod duration;
mod escaping;
mod event;
pub mod follows;
mod glob;
mod grant;
mod group;
mod housekeep;
mod id_token;
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
pub mod merging;
mod nudge;
mod origin;
mod pattern;
mod pending;
mod position;
mod profile;
mod published;
mod queue;
mod rate;
mod readied;
mod render;
pub mod report;
mod reran;
mod rerun;
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
pub use audit::{AuditDay, Audited, Refusals, Refusing, Window, utc_day};
pub use bounds::{
    Attempts, Count, Counted, Inbound, LookedUpPins, PinLookups, ReadBack, ReadBackPages, Sandbox,
    WriteAttempts,
};
pub use bump::{Bump, BumpPattern, Bumper, NotABumpPattern, bumped, read_bump};
pub use chunk::{Room, chunk, size};
pub use claim_pattern::ClaimPattern;
pub use credentials::{basic, bot_or_webhook};
pub use cursor::{Cursor, Polled, Proof, Tombstone, Unheld, fresh};
pub use decide::{
    Answer, Decided, LONGEST_OPTION, MOST_OPTIONS, Options, Question, Slot, decided, state,
};
pub use declined::Declined;
pub use dequeue::{Dequeue, dequeue_event, dequeue_identity, dequeues};
pub use duration::{Duration, Lasting};
pub use escaping::{Escaping, safe_url, segment, unreserved};
pub use event::{Draft, Event, Problem};
pub use glob::{Glob, NotAGlob};
pub use grant::{Action, Grant, until};
pub use group::{
    Change, GROUP_KEY_MOST, GROUP_MOST, GROUP_MOST_ALLOWED, GROUP_WINDOW, Group, GroupLate,
    GroupMode, GroupUntil, Grouping, Subject, group_identity, shown,
};
pub use housekeep::{Chore, Schedule, due, schedules};
pub use id_token::{
    IdToken, KeptKeys, KeySet, RsaKey, TokenAlgorithm, Untaken, discovery_of, jwks_uri, same_origin,
};
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
    BRANCH, CHANGELOG_LINK, COMMIT, CONCLUSION, DRAFT, DUE, ECOSYSTEM, ENTRY_LINK, FROM, HEAD,
    ISSUE_LINK, ISSUES_CLOSED, ISSUES_OPEN, Kind, KindSpec, KindSpecs, LABELS, LinkSpec, MERGED_BY,
    MILESTONE_LINK, NUMBER, PACKAGE, PACKAGE_TYPE, PATH, PRERELEASE, PULL_LINK, PULLS,
    RELEASE_LINK, RESOLUTION, RULE, RUN_LINK, SEVERITY, STALE, STATE_REASON, TOOL, TRIGGER,
    VERSION, VIEW, WORKFLOW, readable, versioned,
};
pub use kind::{
    CALL, CHECK, DAYS, FAILURE, FLIPS, HOLDS, LEFT, MILESTONE_TITLE, REASON, REPOSITORY, RUN_ID,
    SPELL, STAGE, TURN, VERDICT, WAITED, WAITING_ON,
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
pub use nudge::{
    MOST_NUDGES, MOST_STAGES, NUDGE_PREFIX, Nudge, Stages, nudge_event, nudge_identity, nudges,
    pull_identity, stage_name,
};
pub use origin::{Origin, kind_name};
pub use pattern::Pattern;
pub use pending::{HeldEvent, Hold, LONGEST_HOLD, Pending};
pub use position::{position, unparsed};
pub use profile::{
    Bullet, Emphases, Escape, Features, Fences, Heading, Limits, Links, Markup, Measure,
    MentionRoom, MentionStyle, Pins, Profile, Threads,
};
pub use published::{Feed, FeedItem, PeerKeys, Publication, PublishedKeys};
pub use queue::{
    ADVISORY_REPOSITORIES, Advised, AdvisedPost, AdvisedReports, Advising, Advisories,
    AdvisoriesFrom, Advisory, Area, AreaKind, Areas, BLOCKER_LABELS, BaseKey, BaseRead, Based,
    Bases, Batch, Beside, Blocker, Board, Bots, CAPACITY_REPOSITORIES, CHANGESETS_HEAD, CODEOWNERS,
    Capacity, Changed, Checks, Closed, CodeOwner, Codeowners, CodeownersKey, CodeownersRef,
    Combine, Deciding, Dequeued, Described, Dialect, Failure, Flip, Folded, Folding, Given,
    GroupBy, Grouped, Head, Headed, Header, HeaderRead, Held, HeldBy, Holder, Holding, Judged,
    Judging, KEPT_BASE, KEPT_CODEOWNERS, KeptBase, KeptCheck, KeptCodeowners, KeptPull, LEAST_TEAM,
    LOAD_WINDOW, LONGEST_LOAD_WINDOW, Left, Lifecycle, Load, Loaded, Loads, MOST_ADVISED_REPORTS,
    MOST_ADVISORIES, MOST_ADVISORIES_ALLOWED, MOST_ADVISORY_REPOSITORIES, MOST_AREA_GLOB,
    MOST_AREA_GLOBS, MOST_AREA_NAME, MOST_AREA_PATH, MOST_AREAS, MOST_AREAS_READ, MOST_BASES,
    MOST_BLOCKER_LABELS, MOST_CAPACITY_REPOSITORIES, MOST_CODEOWNERS, MOST_CODEOWNERS_LINES,
    MOST_CODEOWNERS_LOOKUPS, MOST_CODEOWNERS_PATTERN, MOST_CODEOWNERS_READ, MOST_FLIPS,
    MOST_FORGE_TEAMS, MOST_HEADER_MILESTONES, MOST_HEADER_REPOSITORIES, MOST_HELD_CHECKS,
    MOST_HOLDS, MOST_ISSUE_ROWS, MOST_KEPT_FAILED, MOST_LEAST_TEAM, MOST_LINE_OWNERS, MOST_LOGIN,
    MOST_RULE_COMPILED, MOST_RULES_COMPILED, MOST_TEAM_MEMBERS, MOST_TEAMS, Mergeable, Milestoned,
    OpenIssue, OpenMilestone, Outcome, Overviews, Owned, Printed, Pull, PullKey, QueueEntry,
    QueueState, RECALLED, RELEASE_PLEASE_HEAD, RULES_CACHE, Ran, Read, Readiness, Reason,
    Reconciled, Release, ReleasePull, RepositoryKey, Review, Reviewed, Row, Rows, Rules, Running,
    SHORTEST_LOAD_WINDOW, Showing, Since, Skipped, Surface, Team, TeamMember, Teams, Triage, Turn,
    Unread, Verdict, View, WITHHELD, Withheld, Zoning, advises, board, grouped, header_asked,
    heading, joined, joined_checks, overlaps, rows, teams_reach, teams_reach_each,
};
pub use rate::{Per, Rate};
pub use readied::{
    Alerting, LATE_VERDICT, MOST_LATE_DAYS, READY_VERDICT, Readied, ReleaseVerdict, Told, readied,
    readied_event, readied_identity, told_of,
};
pub use render::{
    ACTOR_FIELDS, ActorName, Context, Form, ProjectName, REPOSITORY_FIELDS, TemplateError,
    Templates, view_json, waited,
};
pub use report::{
    REPORT_PREFIX, ReportSchedule, living_identity, report_event, report_identity, zone,
};
pub use reran::{Permission, Reran, RerunRecord, RunKey};
pub use rerun::{MOST_RUNS, MOST_RUNS_ALLOWED, RERUN_PREFIX, Rerun, RerunPlan, rerun_event};
pub use retention::Retention;
pub use revision::{
    Edits, LateRelease, NO_PART, Replacing, Revise, Revision, Stray, Withdraw, already_deleted,
    regrouped, replaced, resumes, revise, revised,
};
pub use route::{
    AreaPaths, Delivery, DestinationName, Filter, Mentions, Pinning, Prereleases, Reach, RoleId,
    Route, SecretRef, SourceName, TemplateName, Thread, route,
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
