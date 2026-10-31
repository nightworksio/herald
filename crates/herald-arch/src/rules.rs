use std::fmt;

use crate::layer::Layer;
use crate::workspace::{Kind, Workspace};

const RUNTIME: &[&str] = &[
    "async-std",
    "axum",
    "getrandom",
    "hyper",
    "lambda_runtime",
    "mio",
    "postgres",
    "rand",
    "reqwest",
    "rusqlite",
    "smol",
    "sqlx",
    "tokio",
    "tokio-postgres",
    "worker",
];

/// One way the workspace breaks its layering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// A workspace crate no layer claims.
    Unplaced {
        /// The crate.
        name: String,
    },
    /// A crate naming a crate of a later layer.
    Later {
        /// The naming crate.
        from: String,
        /// The crate of the later layer.
        to: String,
    },
    /// A crate naming another crate of its own layer.
    Sibling {
        /// The naming crate.
        from: String,
        /// The sibling.
        to: String,
    },
    /// An edge to or from the tooling, which stands outside the order.
    Tooling {
        /// The naming crate.
        from: String,
        /// The named crate.
        to: String,
    },
    /// A crate before the I/O layers naming an outside crate its layer does not allow.
    Outside {
        /// The naming crate.
        from: String,
        /// The outside crate.
        to: String,
    },
    /// A crate before the I/O layers reaching an async runtime or an I/O crate.
    Runtime {
        /// The reaching crate.
        from: String,
        /// The runtime or I/O crate.
        to: String,
    },
    /// A crate before the I/O layers with a `clippy.toml` of its own, which would
    /// lift the workspace's bans on ambient I/O.
    OwnClippyConfig {
        /// The crate.
        name: String,
    },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unplaced { name } => write!(
                f,
                "{name} belongs to no layer; name it for its layer, or add it to Layer::of"
            ),
            Self::Later { from, to } => write!(
                f,
                "{from} names {to}, of a later layer; move what it needs into an earlier layer"
            ),
            Self::Sibling { from, to } => write!(
                f,
                "{from} names its sibling {to}; move what they share into an earlier layer"
            ),
            Self::Tooling { from, to } => write!(
                f,
                "{from} names {to}, and the tooling neither names nor is named by a layer"
            ),
            Self::Outside { from, to } => write!(
                f,
                "{from} names {to}, which its layer does not allow; allow it in Layer::allows_outside or drop it"
            ),
            Self::Runtime { from, to } => write!(
                f,
                "{from} reaches {to}, a runtime or I/O crate that only the I/O layers may hold"
            ),
            Self::OwnClippyConfig { name } => write!(
                f,
                "{name} carries its own clippy.toml, which lifts the bans on ambient I/O; delete it"
            ),
        }
    }
}

/// Every violation of the layering in `workspace`, in the order the packages
/// and their dependencies are listed.
#[must_use]
pub fn violations(workspace: &Workspace) -> Vec<Violation> {
    let mut found = Vec::new();
    for package in &workspace.packages {
        let Some(layer) = Layer::of(&package.name) else {
            found.push(Violation::Unplaced {
                name: package.name.clone(),
            });
            continue;
        };
        for dependency in &package.dependencies {
            let from = package.name.clone();
            let to = dependency.name.clone();
            if workspace.has(&dependency.name) {
                found.extend(edge(layer, Layer::of(&dependency.name), from, to));
            } else if dependency.kind != Kind::Dev && !layer.allows_outside(&dependency.name) {
                found.push(Violation::Outside { from, to });
            }
        }
        if !layer.does_io() {
            found.extend(
                package
                    .reaches
                    .iter()
                    .filter(|name| RUNTIME.contains(&name.as_str()))
                    .map(|name| Violation::Runtime {
                        from: package.name.clone(),
                        to: name.clone(),
                    }),
            );
            if package.own_clippy_config {
                found.push(Violation::OwnClippyConfig {
                    name: package.name.clone(),
                });
            }
        }
    }
    found
}

fn edge(from_layer: Layer, to_layer: Option<Layer>, from: String, to: String) -> Option<Violation> {
    let to_layer = to_layer?;
    if from_layer == Layer::Tooling || to_layer == Layer::Tooling {
        return Some(Violation::Tooling { from, to });
    }
    match to_layer.cmp(&from_layer) {
        std::cmp::Ordering::Less => None,
        std::cmp::Ordering::Equal => Some(Violation::Sibling { from, to }),
        std::cmp::Ordering::Greater => Some(Violation::Later { from, to }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::workspace::{Dependency, Package};

    struct Spec<'a> {
        name: &'a str,
        dependencies: &'a [(&'a str, Kind)],
        reaches: &'a [&'a str],
        own_clippy_config: bool,
    }

    fn crate_named(name: &str) -> Spec<'_> {
        Spec {
            name,
            dependencies: &[],
            reaches: &[],
            own_clippy_config: false,
        }
    }

    fn workspace(specs: &[Spec<'_>]) -> Workspace {
        Workspace {
            packages: specs
                .iter()
                .map(|spec| Package {
                    name: spec.name.to_owned(),
                    dependencies: spec
                        .dependencies
                        .iter()
                        .map(|(name, kind)| Dependency {
                            name: (*name).to_owned(),
                            kind: *kind,
                        })
                        .collect(),
                    reaches: spec
                        .reaches
                        .iter()
                        .map(|name| (*name).to_owned())
                        .collect::<BTreeSet<_>>(),
                    own_clippy_config: spec.own_clippy_config,
                })
                .collect(),
        }
    }

    fn edge_of(from: &str, to: &str) -> (String, String) {
        (from.to_owned(), to.to_owned())
    }

    #[test]
    fn a_crate_naming_only_earlier_layers_and_allowed_crates_breaks_nothing() {
        let found = violations(&workspace(&[
            Spec {
                dependencies: &[("sha2", Kind::Normal)],
                ..crate_named("herald-core")
            },
            Spec {
                dependencies: &[("herald-core", Kind::Normal), ("proptest", Kind::Dev)],
                ..crate_named("herald-ports")
            },
        ]));
        assert_eq!(found, []);
    }

    #[test]
    fn a_crate_no_layer_claims_is_unplaced_and_no_edge_to_it_is_judged() {
        let found = violations(&workspace(&[
            Spec {
                dependencies: &[("herald-misc", Kind::Normal)],
                ..crate_named("herald-core")
            },
            crate_named("herald-misc"),
        ]));
        assert_eq!(
            found,
            [Violation::Unplaced {
                name: "herald-misc".to_owned()
            }]
        );
    }

    #[test]
    fn naming_a_later_layer_is_refused() {
        let found = violations(&workspace(&[
            Spec {
                dependencies: &[("herald-ports", Kind::Dev)],
                ..crate_named("herald-core")
            },
            crate_named("herald-ports"),
        ]));
        let (from, to) = edge_of("herald-core", "herald-ports");
        assert_eq!(found, [Violation::Later { from, to }]);
    }

    #[test]
    fn naming_a_sibling_is_refused() {
        let found = violations(&workspace(&[
            Spec {
                dependencies: &[("herald-dest-slack", Kind::Normal)],
                ..crate_named("herald-dest-discord")
            },
            crate_named("herald-dest-slack"),
        ]));
        let (from, to) = edge_of("herald-dest-discord", "herald-dest-slack");
        assert_eq!(found, [Violation::Sibling { from, to }]);
    }

    #[test]
    fn the_tooling_neither_names_nor_is_named() {
        let found = violations(&workspace(&[
            Spec {
                dependencies: &[("herald-arch", Kind::Dev)],
                ..crate_named("herald")
            },
            Spec {
                dependencies: &[("herald-core", Kind::Normal)],
                ..crate_named("herald-arch")
            },
            crate_named("herald-core"),
        ]));
        let (from, to) = edge_of("herald", "herald-arch");
        let (tooling, core) = edge_of("herald-arch", "herald-core");
        assert_eq!(
            found,
            [
                Violation::Tooling { from, to },
                Violation::Tooling {
                    from: tooling,
                    to: core
                }
            ]
        );
    }

    #[test]
    fn an_outside_crate_the_layer_does_not_allow_is_refused_unless_only_for_tests() {
        let found = violations(&workspace(&[Spec {
            dependencies: &[
                ("serde", Kind::Normal),
                ("cc", Kind::Build),
                ("proptest", Kind::Dev),
            ],
            ..crate_named("herald-ports")
        }]));
        let (ports, serde) = edge_of("herald-ports", "serde");
        let (from, cc) = edge_of("herald-ports", "cc");
        assert_eq!(
            found,
            [
                Violation::Outside {
                    from: ports,
                    to: serde
                },
                Violation::Outside { from, to: cc }
            ]
        );
    }

    #[test]
    fn the_io_layers_name_any_outside_crate_and_reach_any_runtime() {
        let found = violations(&workspace(&[Spec {
            dependencies: &[("tokio", Kind::Normal)],
            reaches: &["tokio", "mio"],
            own_clippy_config: true,
            ..crate_named("herald-native")
        }]));
        assert_eq!(found, []);
    }

    #[test]
    fn reaching_a_runtime_before_the_io_layers_is_refused() {
        let found = violations(&workspace(&[Spec {
            reaches: &["serde", "tokio"],
            ..crate_named("herald-core")
        }]));
        let (from, to) = edge_of("herald-core", "tokio");
        assert_eq!(found, [Violation::Runtime { from, to }]);
    }

    #[test]
    fn a_clippy_toml_of_its_own_before_the_io_layers_is_refused() {
        let found = violations(&workspace(&[Spec {
            own_clippy_config: true,
            ..crate_named("herald-core")
        }]));
        assert_eq!(
            found,
            [Violation::OwnClippyConfig {
                name: "herald-core".to_owned()
            }]
        );
    }

    #[test]
    fn every_violation_names_its_crates_and_the_fix() {
        let (from, to) = edge_of("herald-from", "herald-to");
        let cases = [
            (
                Violation::Unplaced { name: from.clone() },
                "add it to Layer::of",
            ),
            (
                Violation::Later {
                    from: from.clone(),
                    to: to.clone(),
                },
                "of a later layer",
            ),
            (
                Violation::Sibling {
                    from: from.clone(),
                    to: to.clone(),
                },
                "its sibling",
            ),
            (
                Violation::Tooling {
                    from: from.clone(),
                    to: to.clone(),
                },
                "the tooling",
            ),
            (
                Violation::Outside {
                    from: from.clone(),
                    to: to.clone(),
                },
                "Layer::allows_outside",
            ),
            (
                Violation::Runtime {
                    from: from.clone(),
                    to: to.clone(),
                },
                "only the I/O layers",
            ),
            (
                Violation::OwnClippyConfig { name: from.clone() },
                "delete it",
            ),
        ];
        for (violation, fix) in cases {
            let sentence = violation.to_string();
            assert!(sentence.starts_with(&from), "{sentence}");
            assert!(sentence.contains(fix), "{sentence}");
            let names_to = matches!(
                violation,
                Violation::Unplaced { .. } | Violation::OwnClippyConfig { .. }
            ) || sentence.contains(&to);
            assert!(names_to, "{sentence}");
        }
    }
}
