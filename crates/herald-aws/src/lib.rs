//! What every herald crate reaching AWS shares (ADR-0001): each request
//! signed with AWS Signature Version 4, for DynamoDB's JSON API and
//! Lambda's invocations alike.

mod sigv4;

pub use sigv4::{Signing, signed, signing_key};
