//! What every herald driver composes (ADR-0017): the config's store
//! reached over the ports a [`Host`] gives, and every route, source and
//! destination connected, to answer requests, do the work they leave, and
//! run each scheduled round. It names no runtime: the native binary, the
//! wasm module and the WASI component each give it their ports.

mod capped;
mod composed;
mod host;
mod records;

pub use capped::Capped;
pub use composed::Composed;
pub use host::{Host, KvAt, SqlAt, Unoffered};
pub use records::{GITHUB_REPOSITORY, GITHUB_TOKEN, Records, Served, reached};
