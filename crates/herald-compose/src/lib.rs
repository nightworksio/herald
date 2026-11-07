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
mod host;
mod hosting;
mod public;
mod reach;
mod reaching;
mod records;
mod round;
pub mod told;

pub use answer::{Answer, Left};
pub use asked::{Asked, Reached, plain};
pub use capped::Capped;
pub use composed::Composed;
pub use host::{Host, KvAt, SqlAt, Unoffered};
pub use hosting::Hosting;
pub use public::{public, reachable};
pub use reach::{Reach, ReachError, SESSIONS, SocketReach};
pub use reaching::{EGRESS_GUARDED, Reaching, asked_query, public_address, received};
pub use records::{GITHUB_REPOSITORY, GITHUB_TOKEN, Records, Served, reached};
pub use round::{ROUND_SECRET, Round};
