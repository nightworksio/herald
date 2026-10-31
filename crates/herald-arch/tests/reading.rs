//! A workspace is read from `cargo metadata` as the rules need it.

use std::fs;
use std::io;
use std::path::Path;

use cargo_metadata::MetadataCommand;
use herald_arch::{Dependency, Kind, Workspace};

fn write(root: &Path, path: &str, contents: &str) -> io::Result<()> {
    let path = root.join(path);
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    fs::write(path, contents)
}

fn member(root: &Path, name: &str, dependencies: &str) -> io::Result<()> {
    write(
        root,
        &format!("{name}/Cargo.toml"),
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n{dependencies}"
        ),
    )?;
    write(root, &format!("{name}/src/lib.rs"), "")
}

#[test]
fn a_workspace_is_read_with_its_kinds_reach_and_clippy_configs() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let root = root.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nresolver = \"3\"\nmembers = [\"app\", \"lib\", \"deep\", \"testkit\", \"builder\"]\n",
    )?;
    member(
        root,
        "app",
        "[dependencies]\nlib = { path = \"../lib\" }\n\n[dev-dependencies]\ntestkit = { path = \"../testkit\" }\n\n[build-dependencies]\nbuilder = { path = \"../builder\" }\n",
    )?;
    member(
        root,
        "lib",
        "[dependencies]\ndeep = { path = \"../deep\" }\n",
    )?;
    member(root, "deep", "")?;
    member(root, "testkit", "")?;
    member(root, "builder", "")?;
    write(root, "deep/clippy.toml", "")?;

    let metadata = MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .other_options(vec!["--offline".to_owned()])
        .exec()
        .map_err(io::Error::other)?;
    let workspace = Workspace::from_metadata(&metadata);

    let app = workspace
        .packages
        .iter()
        .find(|package| package.name == "app")
        .unwrap();
    let declared: Vec<(&str, Kind)> = app
        .dependencies
        .iter()
        .map(|Dependency { name, kind }| (name.as_str(), *kind))
        .collect();
    assert_eq!(
        declared,
        [
            ("lib", Kind::Normal),
            ("testkit", Kind::Dev),
            ("builder", Kind::Build)
        ]
    );
    assert_eq!(
        app.reaches.iter().map(String::as_str).collect::<Vec<_>>(),
        ["builder", "deep", "lib"]
    );
    let configured: Vec<&str> = workspace
        .packages
        .iter()
        .filter(|package| package.own_clippy_config)
        .map(|package| package.name.as_str())
        .collect();
    assert_eq!(configured, ["deep"]);
    assert!(workspace.has("testkit"));
    assert!(!workspace.has("missing"));
    Ok(())
}
