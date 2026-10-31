//! What herald's tests share: a fake of every port, the `Http` that replays
//! recorded fixtures and the recorder that makes them, and the conformance
//! suite every store passes (ADR-0008).
//!
//! It is a dev-dependency only.

#![allow(
    clippy::unused_async_trait_impl,
    reason = "a fake answers at once, and is written as the async trait it fakes"
)]

mod audit;
mod clock;
mod conformance;
mod crypto;
mod fixture;
mod kv;
mod markers;
mod secrets;
mod sql;
mod store;

use std::sync::{Mutex, MutexGuard, PoisonError};

pub use audit::{credentials, secret_header};
pub use clock::FakeClock;
pub use conformance::{Failure, store_conformance};
pub use crypto::{FakeSigner, FakeVerifier};
pub use fixture::{Answer, Exchange, Fixture, Recorded, Recorder, Replay};
pub use kv::FakeKv;
pub use markers::FakeMarkers;
pub use secrets::FakeSecrets;
pub use sql::ScriptedSql;
pub use store::FakeStore;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
