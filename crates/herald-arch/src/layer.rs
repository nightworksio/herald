/// A layer of the workspace, in the order in which layers may name each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// The domain, with no I/O.
    Core,
    /// The traits through which everything outside is reached.
    Ports,
    /// The fakes, the replaying `Http` and the conformance suites.
    Testing,
    /// One crate per source family.
    Sources,
    /// One crate per destination.
    Destinations,
    /// One crate per store.
    Stores,
    /// `herald.toml` read into typed config.
    Config,
    /// The commands as flows over the ports.
    Flows,
    /// The ports implemented for native targets.
    Native,
    /// The composition roots.
    Drivers,
    /// Development tooling, outside the order: it names no layer and no layer
    /// names it.
    Tooling,
}

impl Layer {
    /// The layer the workspace crate `name` belongs to, if any.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        match name {
            "herald-core" => Some(Self::Core),
            "herald-ports" => Some(Self::Ports),
            "herald-testing" => Some(Self::Testing),
            "herald-config" => Some(Self::Config),
            "herald-flows" => Some(Self::Flows),
            "herald-native" => Some(Self::Native),
            "herald" | "herald-worker" | "herald-lambda" | "herald-vercel" | "herald-netlify"
            | "herald-deno" | "herald-server" => Some(Self::Drivers),
            "herald-arch" => Some(Self::Tooling),
            _ if name.starts_with("herald-sources-") => Some(Self::Sources),
            _ if name.starts_with("herald-dest-") => Some(Self::Destinations),
            _ if name.starts_with("herald-store-") => Some(Self::Stores),
            _ => None,
        }
    }

    /// Whether the layer may hold an async runtime and real I/O.
    #[must_use]
    pub fn does_io(self) -> bool {
        matches!(self, Self::Native | Self::Drivers | Self::Tooling)
    }

    /// Whether the layer may name an outside crate directly.
    #[must_use]
    pub fn allows_outside(self, name: &str) -> bool {
        self.does_io() || outside_allowed(self).contains(&name)
    }
}

fn outside_allowed(layer: Layer) -> &'static [&'static str] {
    match layer {
        Layer::Core => &[
            "jiff",
            "minijinja",
            "pulldown-cmark",
            "serde",
            "sha2",
            "unicode-segmentation",
        ],
        Layer::Ports => &["jiff"],
        Layer::Testing => &["jiff", "serde", "serde_json"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_crate_name_belongs_to_its_layer() {
        let cases = [
            ("herald-core", Layer::Core),
            ("herald-ports", Layer::Ports),
            ("herald-testing", Layer::Testing),
            ("herald-sources-forge", Layer::Sources),
            ("herald-dest-discord", Layer::Destinations),
            ("herald-store-sqlite", Layer::Stores),
            ("herald-config", Layer::Config),
            ("herald-flows", Layer::Flows),
            ("herald-native", Layer::Native),
            ("herald", Layer::Drivers),
            ("herald-worker", Layer::Drivers),
            ("herald-lambda", Layer::Drivers),
            ("herald-vercel", Layer::Drivers),
            ("herald-netlify", Layer::Drivers),
            ("herald-deno", Layer::Drivers),
            ("herald-server", Layer::Drivers),
            ("herald-arch", Layer::Tooling),
        ];
        for (name, layer) in cases {
            assert_eq!(Layer::of(name), Some(layer), "{name}");
        }
    }

    #[test]
    fn a_name_no_layer_claims_has_no_layer() {
        assert_eq!(Layer::of("herald-misc"), None);
        assert_eq!(Layer::of("serde"), None);
    }

    #[test]
    fn only_native_drivers_and_tooling_do_io() {
        let io: Vec<Layer> = [
            Layer::Core,
            Layer::Ports,
            Layer::Testing,
            Layer::Sources,
            Layer::Destinations,
            Layer::Stores,
            Layer::Config,
            Layer::Flows,
            Layer::Native,
            Layer::Drivers,
            Layer::Tooling,
        ]
        .into_iter()
        .filter(|layer| layer.does_io())
        .collect();
        assert_eq!(io, [Layer::Native, Layer::Drivers, Layer::Tooling]);
    }

    #[test]
    fn a_layer_before_the_io_layers_names_only_its_allowed_outside_crates() {
        assert!(Layer::Core.allows_outside("sha2"));
        assert!(!Layer::Core.allows_outside("tokio"));
        assert!(!Layer::Ports.allows_outside("sha2"));
        assert!(!Layer::Sources.allows_outside("serde"));
    }

    #[test]
    fn the_io_layers_name_any_outside_crate() {
        assert!(Layer::Native.allows_outside("tokio"));
        assert!(Layer::Drivers.allows_outside("axum"));
        assert!(Layer::Tooling.allows_outside("cargo_metadata"));
    }
}
