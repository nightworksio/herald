#![allow(
    clippy::unused_async_trait_impl,
    reason = "the model answers at once, and is written as the async trait it fakes"
)]

mod activitypub;
mod announce;
mod apps;
mod commands;
mod doctor;
mod doctoring;
mod files;
mod first_run;
mod followers;
mod forges;
mod housekeep;
mod lambda;
mod load;
mod merge_queue;
mod milestones;
mod pages;
mod peers;
mod poll;
mod presets;
mod queue;
mod release;
mod rerun;
mod runs;
mod secrets;
mod server;
mod setup;
mod setup_queue;
mod stores;
mod withdraw;
mod world;
