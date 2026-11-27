//! `herald.toml` read into typed config (ADR-0006).
//!
//! Loading parses the file, reads each section into the types its source,
//! destination or store defines, resolves every name, compiles every
//! template, and dry-renders each route's templates against sample events.
//! Every error in the file is reported in one pass, by line, column and key,
//! with what to write instead.

mod config;
mod error;
mod example;
mod file;
mod files;
mod kinds;
mod load;
mod merging;
mod models;
mod oidc;
mod peers;
mod presets;
mod reference;
mod samples;
mod schema;
mod views;

pub use config::{
    Announce, App, Cli, Config, Doctor, Federation, GithubApp, Guard, HeldSessions, KeyCache,
    Level, Log, LogFormat, Pages, RouteConfig, Run, Schedule, Server, Uses, Work,
};
pub use error::{ConfigError, Errors};
pub use files::{Files, UNCONTAINED, Unread, Within};
pub use herald_kinds::{ConfiguredStore, Destination, Placed, Source};
pub use kinds::Kinds;
pub use load::{expansion, load, load_with};
pub use merging::MergeQueue;
pub use models::{MOST_ASKED, ModelConfig, Task};
pub use oidc::{Oidc, Replay, Taking};
pub use peers::{Peer, Rotation};
pub use presets::{Lands, Parameter, Preset, SHIPPED, SHIPPED_PREFIX, shipped};
pub use reference::reference;
pub use samples::{sample_boards, sample_origin, samples, samples_from};
pub use schema::schema;
pub use views::{
    BUILT_IN_VIEW, DEQUEUED_TEMPLATE, NUDGE_TEMPLATE, QUEUE_TEMPLATE, READINESS_TEMPLATE,
    RECALL_TEMPLATE, RERUN_TEMPLATE, Report, Rule, ViewConfig, ViewMode,
};
