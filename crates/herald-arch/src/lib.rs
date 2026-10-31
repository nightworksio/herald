//! The layering of herald's workspace, held over its dependency graph.
//!
//! [`Workspace::from_metadata`] reads the graph `cargo metadata` describes, and
//! [`violations`] names every edge and dependency the layers refuse.

mod layer;
mod rules;
mod workspace;

pub use layer::Layer;
pub use rules::{Violation, violations};
pub use workspace::{Dependency, Kind, Package, Workspace};
