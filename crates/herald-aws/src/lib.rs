//! What every herald crate reaching AWS shares (ADR-0001): where each
//! service is reached in a region's partition, the variables the platform
//! sets, and each request signed with AWS Signature Version 4, for
//! DynamoDB's JSON API and Lambda's invocations alike.

mod endpoint;
mod sigv4;

pub use endpoint::{Endpoint, configured, endpoint};
pub use sigv4::{Signing, signed};

/// The variable naming the region, as Lambda and the AWS SDKs read it.
pub const REGION: &str = "AWS_REGION";
/// The variable naming the access key's id.
pub const ACCESS_KEY_ID: &str = "AWS_ACCESS_KEY_ID";
/// The variable naming the secret access key.
pub const SECRET_ACCESS_KEY: &str = "AWS_SECRET_ACCESS_KEY";
/// The variable naming a session token, where the credentials are
/// temporary.
pub const SESSION_TOKEN: &str = "AWS_SESSION_TOKEN";
