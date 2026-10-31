use std::collections::{BTreeMap, BTreeSet};

use cargo_metadata::{DependencyKind, Metadata, Node, PackageId};

/// How a package depends on another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A dependency of the package's own code.
    Normal,
    /// A dependency of its tests, examples and benchmarks only.
    Dev,
    /// A dependency of its build script.
    Build,
}

/// One dependency a workspace package declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The depended-on package's name.
    pub name: String,
    /// How it is depended on.
    pub kind: Kind,
}

/// One package of the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The package's name.
    pub name: String,
    /// The dependencies it declares.
    pub dependencies: Vec<Dependency>,
    /// Every package its normal and build dependencies reach, transitively.
    pub reaches: BTreeSet<String>,
    /// Whether its directory carries a `clippy.toml` of its own.
    pub own_clippy_config: bool,
}

/// The workspace's packages, as the rules read them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Workspace {
    /// Every member of the workspace.
    pub packages: Vec<Package>,
}

impl Workspace {
    /// The workspace `cargo metadata` describes.
    #[must_use]
    pub fn from_metadata(metadata: &Metadata) -> Self {
        let names: BTreeMap<&PackageId, String> = metadata
            .packages
            .iter()
            .map(|package| (&package.id, package.name.to_string()))
            .collect();
        let nodes: BTreeMap<&PackageId, &Node> = metadata
            .resolve
            .iter()
            .flat_map(|resolve| &resolve.nodes)
            .map(|node| (&node.id, node))
            .collect();
        let packages = metadata
            .workspace_packages()
            .into_iter()
            .map(|package| Package {
                name: package.name.to_string(),
                dependencies: package
                    .dependencies
                    .iter()
                    .map(|dependency| Dependency {
                        name: dependency.name.clone(),
                        kind: kind(dependency.kind),
                    })
                    .collect(),
                reaches: reaches(&package.id, &nodes, &names),
                own_clippy_config: package
                    .manifest_path
                    .parent()
                    .is_some_and(|directory| directory.join("clippy.toml").is_file()),
            })
            .collect();
        Self { packages }
    }

    /// Whether `name` is a member of the workspace.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.packages.iter().any(|package| package.name == name)
    }
}

fn kind(kind: DependencyKind) -> Kind {
    match kind {
        DependencyKind::Development => Kind::Dev,
        DependencyKind::Build => Kind::Build,
        _ => Kind::Normal,
    }
}

fn reaches(
    root: &PackageId,
    nodes: &BTreeMap<&PackageId, &Node>,
    names: &BTreeMap<&PackageId, String>,
) -> BTreeSet<String> {
    let mut seen: BTreeSet<&PackageId> = BTreeSet::new();
    let mut queue = vec![root];
    while let Some(id) = queue.pop() {
        for dependency in nodes.get(id).into_iter().flat_map(|node| &node.deps) {
            let shipped = dependency
                .dep_kinds
                .iter()
                .any(|info| info.kind != DependencyKind::Development);
            if shipped && seen.insert(&dependency.pkg) {
                queue.push(&dependency.pkg);
            }
        }
    }
    seen.into_iter()
        .filter_map(|id| names.get(id).cloned())
        .collect()
}
