//! The ports: the traits through which herald reaches everything outside its
//! core (ADR-0001).
//!
//! Every method answers a value or an error value, and every future is `Send`.
//! A driver on a single-threaded platform wraps its platform's futures to say
//! so.

mod clock;
mod crypto;
mod http;
mod kv;
mod markers;
mod secret;
mod sql;
mod store;

pub use clock::Clock;
pub use crypto::{Algorithm, CryptoError, Signer, Verifier};
pub use http::{BODY_LIMIT, Http, HttpError, Method, Request, Response, USER_AGENT};
pub use kv::{Kv, KvError, Pair, Stamped, Versionstamp};
pub use markers::{MarkedThread, Markers, MarkersError, marker};
pub use secret::{Secret, SecretError, Secrets, Segment, Text};
pub use sql::{Changed, Sql, SqlError, Statement, Value};
pub use store::{Admitted, Capabilities, Keeping, Store, StoreError, Version, Versioned, Written};
