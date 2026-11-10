//! What every herald driver composes (ADR-0017): the config's store
//! reached over the ports a [`Host`] gives, and every route, source and
//! destination connected, to answer requests, do the work they leave, and
//! run each scheduled round. It names no runtime: the native binary, the
//! wasm module, the WASI component and Fastly Compute each give it their
//! ports.

mod answer;
mod asked;
mod capped;
mod composed;
#[cfg(any(test, feature = "conformance"))]
mod conformance;
mod host;
mod hosting;
mod kinds;
mod public;
mod reach;
mod reaching;
mod records;
pub mod told;

pub use answer::{Answer, Left};
pub use asked::Asked;
pub use capped::Capped;
pub use composed::Composed;
#[cfg(any(test, feature = "conformance"))]
pub use conformance::{
    CONFORMANCE_EMPTY, CONFORMANCE_SECRET, CONFORMANCE_VAR, CONFORMS, host_conformance,
};
pub use herald_kinds::{KvAt, SqlAt};
pub use host::{Host, Unoffered, plain};
pub use hosting::{CRON_SECRET, HOUSEKEPT, Hosting, POLLED};
pub use kinds::kinds;
pub use public::{public, reachable};
pub use reach::{Reach, ReachError, SocketReach};
pub use reaching::{EGRESS_GUARDED, Reaching, asked_query, public_address, received};
pub use records::{GITHUB_REPOSITORY, GITHUB_TOKEN, Opener, Records, Served, reached};
