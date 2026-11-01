//! `herald.toml` read into typed config (ADR-0006).
//!
//! Loading parses the file, reads each section into the types its source,
//! destination or store defines, resolves every name, compiles every
//! template, and dry-renders each route's templates against sample events.
//! Every error in the file is reported in one pass, by line, column and key,
//! with what to write instead.

mod config;
mod error;
mod file;
mod load;
mod position;
mod samples;
mod schema;

pub use config::{App, Config, Destination, RouteConfig, RouteOptions, Schedule, Source, Store};
pub use error::{ConfigError, Errors};
pub use load::load;
pub use samples::samples;
pub use schema::schema;
