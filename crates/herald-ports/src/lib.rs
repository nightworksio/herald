//! The ports: the traits through which herald reaches everything outside its
//! core (ADR-0001).
//!
//! Every method answers a value or an error value, and every future is `Send`.
//! A driver on a single-threaded platform wraps its platform's futures to say
//! so.

mod clock;
mod credential;
mod crypto;
mod dyn_store;
mod erased;
mod http;
mod keyring;
mod kv;
mod learned;
mod markers;
mod outbound;
mod secret;
mod socket;
mod sql;
mod store;
mod timer;

pub use clock::Clock;
pub use credential::Credential;
pub use crypto::{Algorithm, CryptoError, Signer, Verifier};
pub use dyn_store::DynStore;
pub use erased::{Boxed, DynHttp, DynKv, DynMarkers, DynSecrets, DynSigner, DynSql, Erased, Owned};
pub use http::{Http, HttpError, Method, PUBLIC, Request, Response, USER_AGENT, WithBody};
pub use keyring::{Keyring, KnownKey};
pub use kv::{Kv, KvError, Pair, Stamped, Versionstamp};
pub use learned::Learned;
pub use markers::{MarkedThread, Markers, MarkersError, marker};
pub use outbound::{Outbound, Proxy, Roots};
pub use secret::{Secret, SecretError, Secrets, Segment, Text};
pub use socket::{Frame, Socket, Sockets};
pub use sql::{Answers, Changed, Parameters, Sql, SqlError, Statement, Value};
pub use store::{
    Admitted, Announcements, Cursors, Deliveries, Held, Inboxes, Keeper, Keeping, Ledger, Notes,
    Outboxed, Pins, Schedules, Store, StoreError, Subscriptions, Unkept, Version, Versioned,
    Written,
};
pub use timer::Timer;
