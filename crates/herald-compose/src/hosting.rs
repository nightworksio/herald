use herald_config::Store;
use herald_flows::Layer;

/// A host herald runs on as WebAssembly, and what it serves there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hosting {
    /// A Cloudflare Worker.
    Worker,
    /// Deno, or Deno Deploy.
    Deno,
    /// Node or Bun, on a host of its own.
    Node,
    /// A Vercel function.
    Vercel,
    /// A Netlify function.
    Netlify,
    /// A `wasi:http` component host with no database: `wasmtime serve`,
    /// wasmCloud and the like.
    Wasi,
    /// Spin, whose component host offers SQLite and Postgres.
    Spin,
    /// Fastly Compute.
    Fastly,
}

const WORKER: [&str; 6] = [
    Store::CLOUDFLARE_D1,
    Store::DURABLE_OBJECT,
    Store::DYNAMODB,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::REPO_VARIABLES,
];
const DENO: [&str; 5] = [
    Store::DENO_KV,
    Store::DYNAMODB,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::REPO_VARIABLES,
];
const NODE: [&str; 7] = [
    Store::SQLITE,
    Store::POSTGRES,
    Store::DENO_KV,
    Store::DYNAMODB,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::REPO_VARIABLES,
];
const FUNCTIONS: [&str; 5] = [
    Store::POSTGRES,
    Store::DYNAMODB,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::REPO_VARIABLES,
];

const WASI: [&str; 4] = [
    Store::DYNAMODB,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::REPO_VARIABLES,
];
const SPIN: [&str; 6] = [
    Store::SQLITE,
    Store::POSTGRES,
    Store::DYNAMODB,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::REPO_VARIABLES,
];
const FASTLY: [&str; 5] = [
    Store::FASTLY_KV,
    Store::DYNAMODB,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::REPO_VARIABLES,
];

impl Hosting {
    /// Every host.
    pub const ALL: [Self; 8] = [
        Self::Worker,
        Self::Deno,
        Self::Node,
        Self::Vercel,
        Self::Netlify,
        Self::Wasi,
        Self::Spin,
        Self::Fastly,
    ];

    /// The host's name, as its shim names it and runs take leases under.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Deno => "deno",
            Self::Node => "node",
            Self::Vercel => "vercel",
            Self::Netlify => "netlify",
            Self::Wasi => "wasi",
            Self::Spin => "spin",
            Self::Fastly => "fastly",
        }
    }

    /// The host named `name`.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|hosting| hosting.name() == name)
    }

    /// The store kinds the host keeps records in.
    #[must_use]
    pub fn stores(self) -> &'static [&'static str] {
        match self {
            Self::Worker => &WORKER,
            Self::Deno => &DENO,
            Self::Node => &NODE,
            Self::Vercel | Self::Netlify => &FUNCTIONS,
            Self::Wasi => &WASI,
            Self::Spin => &SPIN,
            Self::Fastly => &FASTLY,
        }
    }

    /// What the host serves.
    #[must_use]
    pub fn layer(self) -> Layer<'static> {
        Layer {
            stores: self.stores(),
            endpoint: true,
            sessions: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_host_is_found_by_its_name_and_serves_its_endpoint() {
        for hosting in Hosting::ALL {
            assert_eq!(Hosting::of(hosting.name()), Some(hosting));
            assert!(hosting.layer().endpoint);
            assert!(!hosting.layer().sessions);
            assert!(hosting.stores().contains(&Store::MARKERS));
            assert!(!hosting.stores().contains(&Store::ACTIONS_CACHE));
        }
        assert_eq!(Hosting::of("lambda"), None);
        assert!(Hosting::Worker.stores().contains(&Store::CLOUDFLARE_D1));
        assert!(Hosting::Deno.stores().contains(&Store::DENO_KV));
        assert!(Hosting::Node.stores().contains(&Store::SQLITE));
        assert_eq!(Hosting::Vercel.stores(), Hosting::Netlify.stores());
    }
}
