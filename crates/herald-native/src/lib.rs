//! herald's ports implemented for native targets, which the CLI, the Lambda
//! and the server construct theirs from (ADR-0001).

mod crypto;
mod environment;
mod http;
mod postgres;
mod public;
mod random;
mod reach;
#[cfg(test)]
mod serving;
mod socket;
mod sqlite;
mod timer;

pub use crypto::{RingSigner, RingVerifier};
pub use environment::{EnvSecrets, SystemClock};
pub use http::{BuildError, ConnectError, NativeHttp};
pub use postgres::NativePostgres;
pub use public::{PublicHttp, public};
pub use random::random_secret;
pub use reach::{Reach, ReachError};
pub use socket::{NativeSocket, NativeSockets};
pub use sqlite::NativeSqlite;
pub use timer::TokioTimer;
