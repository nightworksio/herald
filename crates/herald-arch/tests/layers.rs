//! The workspace itself holds its layering.

use std::path::Path;

use cargo_metadata::MetadataCommand;
use herald_arch::{Workspace, violations};

#[test]
fn every_crate_names_only_the_layers_before_it() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(manifest)
        .exec()
        .unwrap();
    let workspace = Workspace::from_metadata(&metadata);
    assert!(workspace.has("herald-core"), "the workspace was not read");
    let found: Vec<String> = violations(&workspace)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(found.is_empty(), "{}", found.join("\n"));
}
