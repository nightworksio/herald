//! herald's flows through fakes of every port: an event announced through
//! every route that takes it, once; each source polled for what it has not
//! seen; and each thread's chores done as they fall due.

#![allow(
    clippy::unused_async_trait_impl,
    reason = "a fake answers at once, and is written as the async trait it fakes"
)]

#[cfg(test)]
mod announce;
#[cfg(test)]
mod doctor;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod housekeep;
#[cfg(test)]
mod marked;
#[cfg(test)]
mod poll;
#[cfg(test)]
mod preview;
