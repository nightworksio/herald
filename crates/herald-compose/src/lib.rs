//! What every herald driver composes (ADR-0017): the config's store
//! reached over the ports a [`Host`] gives, and every route, source and
//! destination connected, to answer requests, do the work they leave, and
//! run each scheduled round. It names no runtime: the native binary, the
//! wasm module and the WASI component each give it their ports.

mod capped;
mod composed;
mod host;
mod hosting;
mod public;
mod reach;
mod records;
pub mod told;

pub use capped::Capped;
pub use composed::Composed;
pub use host::{Host, KvAt, SqlAt, Unoffered};
pub use hosting::Hosting;
pub use public::{public, reachable};
pub use reach::{Reach, ReachError};
pub use records::{GITHUB_REPOSITORY, GITHUB_TOKEN, Records, Served, reached};
