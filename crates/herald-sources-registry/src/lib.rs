//! herald's registry sources (ADR-0003): a new version of a package on
//! Packagist, crates.io, npm or Docker Hub, or a new tag of an image on any
//! OCI registry, becomes an
//! [`Event`](herald_core::Event), parsed without I/O.
//!
//! Registries are polled: each poll is described as a request the flow
//! sends, and the answer it hands back is read into the newest versions.

mod crates_io;
mod docker_hub;
mod kind;
mod namespace;
mod npm;
mod oci;
mod options;
mod package;
mod packagist;

pub use kind::{CratesIoKind, DockerHubKind, NpmKind, PackagistKind, RegistrySource};
pub use oci::{OciKind, OciOptions, OciSource};
pub use options::{Declared, RegistryOptions, TagOrdering, VendorListing};
pub use package::{Package, Registry};
