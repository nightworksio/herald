//! herald's registry sources (ADR-0003): a new version of a package on
//! Packagist, crates.io, npm or Docker Hub becomes an
//! [`Event`](herald_core::Event), parsed without I/O.
//!
//! Registries are polled: each poll is described as a request the flow
//! sends, and the answer it hands back is read into the newest versions.

mod crates_io;
mod docker_hub;
mod npm;
mod options;
mod package;
mod packagist;

pub use options::RegistryOptions;
pub use package::{Package, Registry};
