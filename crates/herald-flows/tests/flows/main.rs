//! herald's flows through fakes of every port: an event announced through
//! every route that takes it, once; each source polled for what it has not
//! seen; and each thread's chores done as they fall due.

#![allow(
    clippy::unused_async_trait_impl,
    reason = "a fake answers at once, and is written as the async trait it fakes"
)]

#[cfg(test)]
mod activitypub;
#[cfg(test)]
mod actors;
#[cfg(test)]
mod advised;
#[cfg(test)]
mod advisories;
#[cfg(test)]
mod announce;
#[cfg(test)]
mod at_once;
#[cfg(test)]
mod audiences;
#[cfg(test)]
mod boards;
#[cfg(test)]
mod capacity;
#[cfg(test)]
mod deciding;
#[cfg(test)]
mod doctor;
#[cfg(test)]
mod doctoring;
#[cfg(test)]
mod endpoint;
#[cfg(test)]
mod feeds;
#[cfg(test)]
mod following;
#[cfg(test)]
mod grouping;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod held;
#[cfg(test)]
mod housekeep;
#[cfg(test)]
mod inbound;
#[cfg(test)]
mod interact;
#[cfg(test)]
mod kept;
#[cfg(test)]
mod load;
#[cfg(test)]
mod marked;
#[cfg(test)]
mod mastodon;
#[cfg(test)]
mod matrix;
#[cfg(test)]
mod orphans;
#[cfg(test)]
mod owner;
#[cfg(test)]
mod poll;
#[cfg(test)]
mod preview;
#[cfg(test)]
mod private_kinds;
#[cfg(test)]
mod projects;
#[cfg(test)]
mod publishing;
#[cfg(test)]
mod pulls;
#[cfg(test)]
mod pushing;
#[cfg(test)]
mod readiness;
#[cfg(test)]
mod recalled;
#[cfg(test)]
mod release_header;
#[cfg(test)]
mod replaced;
#[cfg(test)]
mod reports;
#[cfg(test)]
mod reran;
#[cfg(test)]
mod revise;
#[cfg(test)]
mod scoped;
#[cfg(test)]
mod sealed;
#[cfg(test)]
mod serving;
#[cfg(test)]
mod sessions;
#[cfg(test)]
mod slack;
#[cfg(test)]
mod withdraw;
#[cfg(test)]
mod withheld;
#[cfg(test)]
mod zoned;
