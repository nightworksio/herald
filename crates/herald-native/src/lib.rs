//! herald's ports implemented for native targets, which the CLI, the Lambda
//! and the server construct theirs from (ADR-0001).

mod http;
mod reach;

pub use http::{BuildError, NativeHttp};
pub use reach::{Reach, ReachError};
