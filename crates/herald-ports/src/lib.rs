//! The ports: the traits through which herald reaches everything outside its
//! core (ADR-0001).
//!
//! Every method answers a value or an error value, and every future is `Send`.
//! A driver on a single-threaded platform wraps its platform's futures to say
//! so.

mod clock;
mod crypto;
mod http;
mod keyring;
mod kv;
mod markers;
mod outbound;
mod secret;
mod socket;
mod sql;
mod store;
mod timer;

pub use clock::Clock;
pub use crypto::{Algorithm, CryptoError, Signer, Verifier};
pub use http::{
    BODY_LIMIT, Http, HttpError, Method, PUBLIC, Request, Response, USER_AGENT, WithBody,
};
pub use keyring::{Keyring, KnownKey};
pub use kv::{Kv, KvError, Pair, Stamped, Versionstamp};
pub use markers::{MarkedThread, Markers, MarkersError, marker};
pub use outbound::{Outbound, Proxy, Roots};
pub use secret::{Secret, SecretError, Secrets, Segment, Text};
pub use socket::{Frame, Socket, Sockets};
pub use sql::{Answers, Changed, Sql, SqlError, Statement, Value};
pub use store::{
    Admitted, Announcements, Cursors, Deliveries, Held, Inboxes, Keeper, Keeping, Ledger, Notes,
    Pins, Schedules, Store, StoreError, Subscriptions, Unkept, Version, Versioned, Written,
};
pub use timer::Timer;
