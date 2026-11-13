//! herald's records kept one item per record in an AWS DynamoDB table
//! (ADR-0005), for the Lambda, the CLI and any driver that reaches AWS over
//! HTTP.
//!
//! Each item is keyed by its record's directory and name. Each write takes
//! the next number of the table's write count and names the version it
//! replaces in a condition DynamoDB checks as it writes: that is the
//! compare-and-set, and a record removed and written again never takes a
//! version back. Every request is signed with AWS Signature Version 4.

mod kind;

pub use kind::DynamodbKind;

mod dynamodb;
mod options;

pub use dynamodb::{Credentials, Dynamodb, DynamodbRecords, DynamodbStore};
pub use options::{StoreOptions, TABLE};
