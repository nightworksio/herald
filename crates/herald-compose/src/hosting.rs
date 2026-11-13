use herald_flows::{Cadence, Egress, Layer, Memory, ROUND_SECRET, Rounds, Tier};
use herald_kinds::Offer;
use jiff::SignedDuration;

use crate::kinds::kinds;

/// A host herald runs on, as `herald doctor --layer` names it: what runs
/// there, the stores it offers, and how it is asked for rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hosting {
    /// The CLI, on GitHub Actions or any other CI.
    Actions,
    /// A Cloudflare Worker.
    Worker,
    /// Deno, or Deno Deploy.
    Deno,
    /// A Vercel function.
    Vercel,
    /// A Netlify function.
    Netlify,
    /// `herald lambda`, as an AWS Lambda function.
    Lambda,
    /// `herald server` as an Azure Functions custom handler.
    Azure,
    /// A `wasi:http` component host with no database: `wasmtime serve`,
    /// wasmCloud and the like.
    Wasi,
    /// Spin, whose component host offers SQLite and Postgres.
    Spin,
    /// Fastly Compute.
    Fastly,
    /// Node or Bun, on a host of its own.
    Node,
    /// `herald server`, in a container or on any host.
    Server,
}

/// What herald runs as on a host (ADR-0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Artefact {
    /// The native binary.
    Native,
    /// The wasm module, behind a JavaScript shim.
    Module,
    /// The `wasi:http` component.
    Component,
    /// The Fastly Compute service.
    Compute,
}

/// How often the deploy files ask for a poll where a platform's scheduler
/// asks for rounds.
pub const POLLED: SignedDuration = SignedDuration::from_mins(5);
/// How often the deploy files ask for housekeeping where a platform's
/// scheduler asks for rounds.
pub const HOUSEKEPT: SignedDuration = SignedDuration::from_hours(1);

/// The secret Vercel's cron sends, which herald takes as the round secret
/// where that is unset.
pub const CRON_SECRET: &str = "CRON_SECRET";

/// The rounds a host's own scheduler asks for: a poll every `poll`, and
/// housekeeping every `housekeep`.
const fn platform(by: &'static str, poll: SignedDuration, housekeep: SignedDuration) -> Rounds {
    Rounds::Platform(Cadence {
        by,
        poll,
        housekeep,
    })
}

/// The rounds a host's own scheduler asks for at the shipped cadence.
const fn shipped(by: &'static str) -> Rounds {
    platform(by, POLLED, HOUSEKEPT)
}

impl Hosting {
    /// Every host, in ADR-0002's order: those with no endpoint, the
    /// serverless ones, and those that hold their endpoint open.
    pub const ALL: [Self; 12] = [
        Self::Actions,
        Self::Worker,
        Self::Deno,
        Self::Vercel,
        Self::Netlify,
        Self::Lambda,
        Self::Azure,
        Self::Wasi,
        Self::Spin,
        Self::Fastly,
        Self::Node,
        Self::Server,
    ];

    /// The host's name, as `herald doctor --layer` and its shim name it, and
    /// as runs on it take leases under.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Actions => "actions",
            Self::Worker => "worker",
            Self::Deno => "deno",
            Self::Vercel => "vercel",
            Self::Netlify => "netlify",
            Self::Lambda => "lambda",
            Self::Azure => "azure",
            Self::Wasi => "wasi",
            Self::Spin => "spin",
            Self::Fastly => "fastly",
            Self::Node => "node",
            Self::Server => "server",
        }
    }

    /// What the host is, as `herald doctor --help` names it.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Actions => "The CLI, on GitHub Actions or anywhere else",
            Self::Worker => "The wasm module on a Cloudflare Worker",
            Self::Deno => "The wasm module on Deno",
            Self::Vercel => "The wasm module as a Vercel function",
            Self::Netlify => "The wasm module as a Netlify function",
            Self::Lambda => "`herald lambda`, as an AWS Lambda function",
            Self::Azure => "`herald server` as an Azure Functions custom handler",
            Self::Wasi => "The WASI component on a `wasi:http` host",
            Self::Spin => "The WASI component on Spin",
            Self::Fastly => "herald on Fastly Compute",
            Self::Node => "The wasm module on Node or Bun",
            Self::Server => "`herald server`, in a container or on any host",
        }
    }

    /// The host named `name` that `artefact` runs on.
    ///
    /// # Errors
    ///
    /// Where `artefact` runs on no host of that name, naming those it runs
    /// on.
    pub fn running(name: &str, artefact: Artefact) -> Result<Self, String> {
        let ran = || {
            Self::ALL
                .into_iter()
                .filter(move |hosting| hosting.artefact() == artefact)
        };
        ran().find(|hosting| hosting.name() == name).ok_or_else(|| {
            let named: Vec<&str> = ran().map(Self::name).collect();
            format!(
                "{name} is no host herald knows; it runs on {}",
                named.join(", ")
            )
        })
    }

    /// What herald runs as there.
    #[must_use]
    pub const fn artefact(self) -> Artefact {
        match self {
            Self::Actions | Self::Lambda | Self::Azure | Self::Server => Artefact::Native,
            Self::Worker | Self::Deno | Self::Vercel | Self::Netlify | Self::Node => {
                Artefact::Module
            }
            Self::Wasi | Self::Spin => Artefact::Component,
            Self::Fastly => Artefact::Compute,
        }
    }

    /// What it offers a store beyond `Http`.
    #[must_use]
    pub const fn offered(self) -> &'static [Offer] {
        match self {
            Self::Actions => &[Offer::Sqlite, Offer::Postgres, Offer::WorkflowCache],
            Self::Worker => &[Offer::D1, Offer::DurableObject],
            Self::Deno => &[Offer::DenoKv],
            Self::Vercel | Self::Netlify | Self::Lambda => &[Offer::Postgres],
            Self::Azure | Self::Spin | Self::Server => &[Offer::Sqlite, Offer::Postgres],
            Self::Wasi => &[],
            Self::Fastly => &[Offer::FastlyKv],
            Self::Node => &[Offer::Sqlite, Offer::Postgres, Offer::DenoKv],
        }
    }

    /// Whether it offers what `offer` names: `Http` everywhere, and the rest
    /// where [`Hosting::offered`] holds it.
    #[must_use]
    pub fn offers(self, offer: Offer) -> bool {
        offer == Offer::Http || self.offered().contains(&offer)
    }

    /// The store kinds it keeps records in: every kind whose needs it
    /// offers.
    #[must_use]
    pub fn stores(self) -> Vec<&'static str> {
        kinds().store_kinds_offered(|offer| self.offers(offer))
    }

    /// The layer of ADR-0002 it serves.
    #[must_use]
    pub const fn tier(self) -> Tier {
        match self {
            Self::Actions => Tier::Actions,
            Self::Node | Self::Server => Tier::Container,
            Self::Worker
            | Self::Deno
            | Self::Vercel
            | Self::Netlify
            | Self::Lambda
            | Self::Azure
            | Self::Wasi
            | Self::Spin
            | Self::Fastly => Tier::Serverless,
        }
    }

    /// Whether it serves herald's endpoint, where commands are posted.
    #[must_use]
    pub const fn endpoint(self) -> bool {
        !matches!(self, Self::Actions)
    }

    /// Whether it holds sessions open: `herald server` holds them, and on a
    /// Worker its Durable Object does (ADR-0017).
    #[must_use]
    pub const fn sessions(self) -> bool {
        matches!(self, Self::Worker | Self::Server)
    }

    /// How it reaches what herald fetches on others' behalf (ADR-0017).
    #[must_use]
    pub const fn egress(self) -> Egress {
        match self {
            Self::Wasi | Self::Spin | Self::Fastly => Egress::Unchecked,
            Self::Actions
            | Self::Worker
            | Self::Deno
            | Self::Vercel
            | Self::Netlify
            | Self::Lambda
            | Self::Azure
            | Self::Node
            | Self::Server => Egress::Checked,
        }
    }

    /// What asks it for rounds, as its deploy files ship.
    #[must_use]
    pub const fn rounds(self) -> Rounds {
        match self {
            Self::Actions => Rounds::Workflow,
            Self::Worker => shipped("the cron triggers in deploy/worker/wrangler.toml"),
            Self::Deno => shipped("Deno.cron in deploy/deno/main.ts"),
            Self::Vercel => shipped("Vercel cron, as deploy/vercel/vercel.json sets it,"),
            Self::Netlify => shipped("the scheduled functions in deploy/netlify/functions"),
            Self::Lambda => platform(
                "the Rounds schedule in deploy/lambda/template.yaml",
                POLLED,
                POLLED,
            ),
            Self::Azure => shipped("the poll and housekeep timer triggers in deploy/azure"),
            Self::Node | Self::Server => Rounds::Scheduled,
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
            Self::Actions
            | Self::Worker
            | Self::Deno
            | Self::Netlify
            | Self::Lambda
            | Self::Azure
            | Self::Node
            | Self::Server => &[],
        }
    }

    /// How long what it holds in memory lasts.
    #[must_use]
    pub const fn keyring(self) -> Memory {
        match self {
            Self::Azure | Self::Node | Self::Server => Memory::Lasting,
            Self::Actions
            | Self::Worker
            | Self::Deno
            | Self::Vercel
            | Self::Netlify
            | Self::Lambda
            | Self::Wasi
            | Self::Spin
            | Self::Fastly => Memory::PerRequest,
        }
    }

    /// What the host serves.
    #[must_use]
    pub fn layer(self) -> Layer {
        Layer {
            name: self.name(),
            tier: self.tier(),
            stores: self.stores(),
            endpoint: self.endpoint(),
            sessions: self.sessions(),
            egress: self.egress(),
            rounds: self.rounds(),
            round_secrets: self.round_secrets(),
            keyring: self.keyring(),
        }
    }

    /// Every host's layer, in ADR-0002's order.
    #[must_use]
    pub fn layers() -> Vec<Layer> {
        Self::ALL.into_iter().map(Self::layer).collect()
    }
}

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;

    use super::*;

    const HTTP: [&str; 4] = ["dynamodb", "git-branch", "markers", "repo-variables"];

    fn with(offered: &[&'static str]) -> Vec<&'static str> {
        offered.iter().copied().chain(HTTP).collect()
    }

    #[test]
    fn each_host_is_found_by_its_name_and_serves_what_it_describes() {
        for hosting in Hosting::ALL {
            assert_eq!(
                Hosting::running(hosting.name(), hosting.artefact()),
                Ok(hosting)
            );
            assert_ne!(hosting.description(), "");
            assert!(hosting.offers(Offer::Http));
            assert_eq!(hosting.endpoint(), hosting != Hosting::Actions);
            assert_eq!(
                hosting.layer(),
                Layer {
                    name: hosting.name(),
                    tier: hosting.tier(),
                    stores: hosting.stores(),
                    endpoint: hosting.endpoint(),
                    sessions: hosting.sessions(),
                    egress: hosting.egress(),
                    rounds: hosting.rounds(),
                    round_secrets: hosting.round_secrets(),
                    keyring: hosting.keyring(),
                }
            );
        }
        assert_eq!(Hosting::layers(), Hosting::ALL.map(Hosting::layer).to_vec());
        assert_eq!(
            Hosting::running("cloud", Artefact::Native),
            Err(
                "cloud is no host herald knows; it runs on actions, lambda, azure, server"
                    .to_owned()
            )
        );
        let sessions: Vec<Hosting> = Hosting::ALL
            .into_iter()
            .filter(|hosting| hosting.sessions())
            .collect();
        assert_eq!(sessions, [Hosting::Worker, Hosting::Server]);
    }

    #[test]
    fn an_artefact_runs_only_on_its_own_hosts() {
        assert_eq!(
            Hosting::running("server", Artefact::Module),
            Err(
                "server is no host herald knows; it runs on worker, deno, vercel, netlify, node"
                    .to_owned()
            )
        );
        assert_eq!(
            Hosting::running("worker", Artefact::Component),
            Err("worker is no host herald knows; it runs on wasi, spin".to_owned())
        );
        assert_eq!(
            Hosting::running("spin", Artefact::Compute),
            Err("spin is no host herald knows; it runs on fastly".to_owned())
        );
        assert_eq!(
            Hosting::running("wasi", Artefact::Native),
            Err(
                "wasi is no host herald knows; it runs on actions, lambda, azure, server"
                    .to_owned()
            )
        );
    }

    #[test]
    fn each_host_keeps_records_in_every_store_whose_needs_it_offers() {
        let kept = [
            (
                Hosting::Actions,
                vec![
                    "sqlite",
                    "postgres",
                    "dynamodb",
                    "git-branch",
                    "markers",
                    "actions-cache",
                    "repo-variables",
                ],
            ),
            (Hosting::Worker, with(&["d1", "durable-object"])),
            (Hosting::Deno, with(&["deno-kv"])),
            (Hosting::Vercel, with(&["postgres"])),
            (Hosting::Netlify, with(&["postgres"])),
            (Hosting::Lambda, with(&["postgres"])),
            (Hosting::Azure, with(&["sqlite", "postgres"])),
            (Hosting::Wasi, with(&[])),
            (Hosting::Spin, with(&["sqlite", "postgres"])),
            (Hosting::Fastly, with(&["fastly-kv"])),
            (Hosting::Node, with(&["sqlite", "postgres", "deno-kv"])),
            (Hosting::Server, with(&["sqlite", "postgres"])),
        ];
        for (hosting, stores) in kept {
            assert_eq!(hosting.stores(), stores, "{}", hosting.name());
        }
        assert!(!Hosting::Server.offers(Offer::WorkflowCache));
        assert!(Hosting::Actions.offers(Offer::WorkflowCache));
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
        for hosting in [
            Hosting::Worker,
            Hosting::Deno,
            Hosting::Netlify,
            Hosting::Lambda,
        ] {
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
            described(Hosting::Azure),
            (Tier::Serverless, Egress::Checked, &[][..], Memory::Lasting)
        );
        let held = (Tier::Container, Egress::Checked, &[][..], Memory::Lasting);
        for hosting in [Hosting::Node, Hosting::Server] {
            assert_eq!(described(hosting), held);
            assert_eq!(hosting.rounds(), Rounds::Scheduled);
        }
        assert_eq!(
            described(Hosting::Actions),
            (Tier::Actions, Egress::Checked, &[][..], Memory::PerRequest)
        );
        assert_eq!(Hosting::Actions.rounds(), Rounds::Workflow);
        assert_eq!(
            Hosting::Lambda.rounds(),
            Rounds::Platform(Cadence {
                by: "the Rounds schedule in deploy/lambda/template.yaml",
                poll: POLLED,
                housekeep: POLLED,
            })
        );
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
            let asked = matches!(
                hosting.rounds(),
                Rounds::Platform(cadence) if cadence.by.contains(path)
                    && (cadence.poll, cadence.housekeep) == (POLLED, HOUSEKEPT)
            );
            assert!(asked, "{}", hosting.name());
            let text = files.concat();
            assert!(text.contains("\"*/5 * * * *\""), "{path}");
            assert!(text.contains("\"17 * * * *\""), "{path}");
        }
    }

    #[test]
    fn the_deploy_readme_lists_the_stores_each_host_keeps_records_in() {
        let readme = include_str!("../../../deploy/README.md");
        let shims = [
            (Hosting::Worker, "`worker/`"),
            (Hosting::Deno, "`deno/`"),
            (Hosting::Node, "`node/`"),
            (Hosting::Vercel, "`vercel/`"),
            (Hosting::Netlify, "`netlify/`"),
            (Hosting::Lambda, "`lambda/`"),
            (Hosting::Azure, "`azure/`"),
            (Hosting::Wasi, "`component/`"),
            (Hosting::Spin, "`spin/`"),
            (Hosting::Fastly, "`fastly/`"),
            (Hosting::Server, "`compose/`, `kubernetes/`, `helm/herald/`"),
        ];
        for (hosting, shim) in shims {
            let row: Vec<&str> = readme
                .lines()
                .map(|line| line.split('|').map(str::trim).collect::<Vec<_>>())
                .find(|cells| cells.get(2) == Some(&shim))
                .unwrap_or_default();
            let listed: Vec<String> = hosting
                .stores()
                .iter()
                .map(|store| format!("`{store}`"))
                .collect();
            assert_eq!(
                row.get(3).copied(),
                Some(listed.join(", ").as_str()),
                "{shim}"
            );
        }
    }
}
