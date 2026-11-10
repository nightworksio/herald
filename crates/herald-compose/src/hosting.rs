use herald_flows::Layer;
use herald_kinds::StoreKind;
use herald_store_d1::D1Kind;
use herald_store_deno_kv::DenoKvKind;
use herald_store_durable_object::DurableObjectKind;
use herald_store_dynamodb::DynamodbKind;
use herald_store_fastly_kv::FastlyKvKind;
use herald_store_git_branch::GitBranchKind;
use herald_store_markers::MarkersKind;
use herald_store_postgres::PostgresKind;
use herald_store_repo_variables::RepoVariablesKind;
use herald_store_sqlite::SqliteKind;

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
    D1Kind::NAME,
    DurableObjectKind::NAME,
    DynamodbKind::NAME,
    GitBranchKind::NAME,
    MarkersKind::NAME,
    RepoVariablesKind::NAME,
];
const DENO: [&str; 5] = [
    DenoKvKind::NAME,
    DynamodbKind::NAME,
    GitBranchKind::NAME,
    MarkersKind::NAME,
    RepoVariablesKind::NAME,
];
const NODE: [&str; 7] = [
    SqliteKind::NAME,
    PostgresKind::NAME,
    DenoKvKind::NAME,
    DynamodbKind::NAME,
    GitBranchKind::NAME,
    MarkersKind::NAME,
    RepoVariablesKind::NAME,
];
const FUNCTIONS: [&str; 5] = [
    PostgresKind::NAME,
    DynamodbKind::NAME,
    GitBranchKind::NAME,
    MarkersKind::NAME,
    RepoVariablesKind::NAME,
];

const WASI: [&str; 4] = [
    DynamodbKind::NAME,
    GitBranchKind::NAME,
    MarkersKind::NAME,
    RepoVariablesKind::NAME,
];
const SPIN: [&str; 6] = [
    SqliteKind::NAME,
    PostgresKind::NAME,
    DynamodbKind::NAME,
    GitBranchKind::NAME,
    MarkersKind::NAME,
    RepoVariablesKind::NAME,
];
const FASTLY: [&str; 5] = [
    FastlyKvKind::NAME,
    DynamodbKind::NAME,
    GitBranchKind::NAME,
    MarkersKind::NAME,
    RepoVariablesKind::NAME,
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
    use herald_store_actions_cache::ActionsCacheKind;

    use super::*;

    #[test]
    fn each_host_is_found_by_its_name_and_serves_its_endpoint() {
        for hosting in Hosting::ALL {
            assert_eq!(Hosting::of(hosting.name()), Some(hosting));
            assert!(hosting.layer().endpoint);
            assert!(!hosting.layer().sessions);
            assert!(hosting.stores().contains(&MarkersKind::NAME));
            assert!(!hosting.stores().contains(&ActionsCacheKind::NAME));
        }
        assert_eq!(Hosting::of("lambda"), None);
        assert!(Hosting::Worker.stores().contains(&D1Kind::NAME));
        assert!(Hosting::Deno.stores().contains(&DenoKvKind::NAME));
        assert!(Hosting::Node.stores().contains(&SqliteKind::NAME));
        assert_eq!(Hosting::Vercel.stores(), Hosting::Netlify.stores());
    }
}
