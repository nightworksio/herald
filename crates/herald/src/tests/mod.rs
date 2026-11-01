#![allow(
    clippy::unused_async_trait_impl,
    reason = "the model answers at once, and is written as the async trait it fakes"
)]

mod announce;
mod doctor;
mod housekeep;
mod poll;
mod stores;
mod world;
