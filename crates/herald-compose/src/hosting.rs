use herald_flows::{Cadence, Egress, Layer, Memory, ROUND_SECRET, Rounds, Tier};
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
use jiff::SignedDuration;

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

/// How often the deploy files ask for a poll where a platform's scheduler
/// asks for rounds.
pub const POLLED: SignedDuration = SignedDuration::from_mins(5);
/// How often the deploy files ask for housekeeping where a platform's
/// scheduler asks for rounds.
pub const HOUSEKEPT: SignedDuration = SignedDuration::from_hours(1);

/// The secret Vercel's cron sends, which herald takes as the round secret
/// where that is unset.
pub const CRON_SECRET: &str = "CRON_SECRET";

/// The rounds a host's own scheduler asks for at the shipped cadence.
const fn shipped(by: &'static str) -> Rounds {
    Rounds::Platform(Cadence {
        by,
        poll: POLLED,
        housekeep: HOUSEKEPT,
    })
}

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

    /// The layer of ADR-0002 it serves.
    #[must_use]
    pub const fn tier(self) -> Tier {
        match self {
            Self::Node => Tier::Container,
            Self::Worker
            | Self::Deno
            | Self::Vercel
            | Self::Netlify
            | Self::Wasi
            | Self::Spin
            | Self::Fastly => Tier::Serverless,
        }
    }

    /// Whether it holds sessions open: the Worker's Durable Object holds
    /// them (ADR-0017).
    #[must_use]
    pub const fn sessions(self) -> bool {
        matches!(self, Self::Worker)
    }

    /// How it reaches what herald fetches on others' behalf (ADR-0017).
    #[must_use]
    pub const fn egress(self) -> Egress {
        match self {
            Self::Wasi | Self::Spin | Self::Fastly => Egress::Unchecked,
            Self::Worker | Self::Deno | Self::Node | Self::Vercel | Self::Netlify => {
                Egress::Checked
            }
        }
    }

    /// What asks it for rounds, as its deploy files ship.
    #[must_use]
    pub const fn rounds(self) -> Rounds {
        match self {
            Self::Worker => shipped("the cron triggers in deploy/worker/wrangler.toml"),
            Self::Deno => shipped("Deno.cron in deploy/deno/main.ts"),
            Self::Vercel => shipped("Vercel cron, as deploy/vercel/vercel.json sets it,"),
            Self::Netlify => shipped("the scheduled functions in deploy/netlify/functions"),
            Self::Node => Rounds::Scheduled,
            Self::Wasi | Self::Spin | Self::Fastly => Rounds::Asked,
        }
    }

    /// The secrets, any one of which a request asking it for a round holds,
    /// where its rounds are asked for over HTTP alone.
    #[must_use]
    pub const fn round_secrets(self) -> &'static [&'static str] {
        match self {
            Self::Vercel => &[ROUND_SECRET, CRON_SECRET],
            Self::Wasi | Self::Spin | Self::Fastly => &[ROUND_SECRET],
            Self::Worker | Self::Deno | Self::Node | Self::Netlify => &[],
        }
    }

    /// How long what it holds in memory lasts.
    #[must_use]
    pub const fn keyring(self) -> Memory {
        match self {
            Self::Node => Memory::Lasting,
            Self::Worker
            | Self::Deno
            | Self::Vercel
            | Self::Netlify
            | Self::Wasi
            | Self::Spin
            | Self::Fastly => Memory::PerRequest,
        }
    }

    /// What the host serves.
    #[must_use]
    pub fn layer(self) -> Layer<'static> {
        Layer {
            name: self.name(),
            tier: self.tier(),
            stores: self.stores(),
            endpoint: true,
            sessions: self.sessions(),
            egress: self.egress(),
            rounds: self.rounds(),
            round_secrets: self.round_secrets(),
            keyring: self.keyring(),
        }
    }
}

#[cfg(test)]
mod tests {
    use herald_store_actions_cache::ActionsCacheKind;
    use jiff::SignedDuration;

    use super::*;

    #[test]
    fn each_host_is_found_by_its_name_and_serves_its_endpoint() {
        for hosting in Hosting::ALL {
            assert_eq!(Hosting::of(hosting.name()), Some(hosting));
            let layer = hosting.layer();
            assert!(layer.endpoint);
            assert_eq!(layer.sessions, hosting == Hosting::Worker);
            assert_eq!(layer.name, hosting.name());
            assert_eq!(
                layer,
                Layer {
                    name: hosting.name(),
                    tier: hosting.tier(),
                    stores: hosting.stores(),
                    endpoint: true,
                    sessions: hosting.sessions(),
                    egress: hosting.egress(),
                    rounds: hosting.rounds(),
                    round_secrets: hosting.round_secrets(),
                    keyring: hosting.keyring(),
                }
            );
            assert!(hosting.stores().contains(&MarkersKind::NAME));
            assert!(!hosting.stores().contains(&ActionsCacheKind::NAME));
        }
        assert_eq!(Hosting::of("lambda"), None);
        assert!(Hosting::Worker.stores().contains(&D1Kind::NAME));
        assert!(Hosting::Deno.stores().contains(&DenoKvKind::NAME));
        assert!(Hosting::Node.stores().contains(&SqliteKind::NAME));
        assert_eq!(Hosting::Vercel.stores(), Hosting::Netlify.stores());
    }

    #[test]
    fn each_host_names_its_layer_its_egress_and_what_asks_it_for_rounds() {
        let described = |hosting: Hosting| {
            (
                hosting.tier(),
                hosting.egress(),
                hosting.round_secrets(),
                hosting.keyring(),
            )
        };
        let unchecked = (
            Tier::Serverless,
            Egress::Unchecked,
            &[ROUND_SECRET][..],
            Memory::PerRequest,
        );
        for hosting in [Hosting::Wasi, Hosting::Spin, Hosting::Fastly] {
            assert_eq!(described(hosting), unchecked);
            assert_eq!(hosting.rounds(), Rounds::Asked);
        }
        let served = (
            Tier::Serverless,
            Egress::Checked,
            &[][..],
            Memory::PerRequest,
        );
        for hosting in [Hosting::Worker, Hosting::Deno, Hosting::Netlify] {
            assert_eq!(described(hosting), served);
        }
        assert_eq!(
            described(Hosting::Vercel),
            (
                Tier::Serverless,
                Egress::Checked,
                &[ROUND_SECRET, CRON_SECRET][..],
                Memory::PerRequest
            )
        );
        assert_eq!(
            described(Hosting::Node),
            (Tier::Container, Egress::Checked, &[][..], Memory::Lasting)
        );
        assert_eq!(Hosting::Node.rounds(), Rounds::Scheduled);
        assert_eq!(POLLED, SignedDuration::from_mins(5));
        assert_eq!(HOUSEKEPT, SignedDuration::from_hours(1));
    }

    #[test]
    fn each_platform_s_deploy_files_ask_for_rounds_as_its_host_says() {
        let shipped: [(Hosting, &str, &[&str]); 4] = [
            (
                Hosting::Worker,
                "deploy/worker/wrangler.toml",
                &[include_str!("../../../deploy/worker/wrangler.toml")],
            ),
            (
                Hosting::Deno,
                "deploy/deno/main.ts",
                &[include_str!("../../../deploy/deno/main.ts")],
            ),
            (
                Hosting::Vercel,
                "deploy/vercel/vercel.json",
                &[include_str!("../../../deploy/vercel/vercel.json")],
            ),
            (
                Hosting::Netlify,
                "deploy/netlify/functions",
                &[
                    include_str!("../../../deploy/netlify/functions/poll.mjs"),
                    include_str!("../../../deploy/netlify/functions/housekeep.mjs"),
                ],
            ),
        ];
        for (hosting, path, files) in shipped {
            let Rounds::Platform(cadence) = hosting.rounds() else {
                panic!("{} asks for rounds on its platform", hosting.name());
            };
            assert!(cadence.by.contains(path), "{}", cadence.by);
            assert_eq!((cadence.poll, cadence.housekeep), (POLLED, HOUSEKEPT));
            let text = files.concat();
            assert!(text.contains("\"*/5 * * * *\""), "{path}");
            assert!(text.contains("\"17 * * * *\""), "{path}");
        }
    }
}
