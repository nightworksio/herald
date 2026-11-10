//! herald's command line (ADR-0009): its commands, their arguments, and the
//! layer each serves.

use clap::{Parser, Subcommand, ValueEnum};
use herald_sources_forge::Forge;

use herald_compose::{HOUSEKEPT, Hosting, POLLED};
use herald_flows::{Cadence, Egress, Memory, Rounds, Tier};

use super::doctoring::Asked;
use super::stores::{LAMBDA, SERVED, STORES};

#[derive(Debug, Parser)]
#[command(
    name = "herald",
    version,
    about = "Announce what happened to the places people gather."
)]
pub(crate) struct Cli {
    /// The config to read.
    #[arg(
        long,
        global = true,
        env = "HERALD_CONFIG",
        default_value = "herald.toml"
    )]
    pub(crate) config: String,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Print a new marker secret, once, to store as `HERALD_MARKER_SECRET`.
    Init,
    /// Make tokens for the HTTP API's apps.
    #[command(subcommand)]
    Token(Tokens),
    /// Ask a running `herald server` whether it is healthy, as a container's
    /// health check does: exit 0 where it answers, 1 where it does not.
    Health {
        /// The URL to ask, in place of `/healthz` on the address
        /// `HERALD_LISTEN` or the config names.
        #[arg(long)]
        url: Option<String>,
        /// How many milliseconds the server has to answer.
        #[arg(long, default_value_t = 5000)]
        timeout: u64,
    },
    #[command(flatten)]
    Read(Reading),
}

/// The commands that make tokens.
#[derive(Debug, Subcommand)]
pub(crate) enum Tokens {
    /// Print a new token for an app, once, with the hash herald.toml keeps.
    New {
        /// The app's name, as `[apps.<name>]` names it.
        #[arg(long)]
        app: String,
    },
}

/// The commands that read the config.
#[derive(Debug, Subcommand)]
pub(crate) enum Reading {
    #[command(flatten)]
    Serve(Serve),
    /// Check the config, and name what it asks for that herald cannot serve.
    Doctor(Asked),
    /// Print the kind of store the config keeps its records in, as
    /// `[store] kind` writes it.
    Store,
    #[command(flatten)]
    Set(Setting),
    /// Render each route's templates as its destination would show them,
    /// against a sample event or a release event's file.
    Preview {
        /// Only this route.
        #[arg(long)]
        route: Option<String>,
        /// A release event's file, as the forge sends it, in place of a
        /// sample event.
        #[arg(long)]
        event: Option<String>,
        /// The forge whose release event the file is.
        #[arg(long, value_enum, default_value_t = From::Github)]
        from: From,
    },
}

/// The commands that set a destination up to take commands.
#[derive(Debug, Subcommand)]
pub(crate) enum Setting {
    /// Set a Discord destination up to take commands.
    #[command(subcommand)]
    Discord(Discording),
    /// Set a Slack destination up to take commands.
    #[command(subcommand)]
    Slack(Slacking),
    /// Set a Matrix destination up to take commands.
    #[command(subcommand)]
    Matrix(Matrixing),
}

/// The commands that set Discord up to take commands.
#[derive(Debug, Subcommand)]
pub(crate) enum Discording {
    /// Register the commands the destination answers with Discord.
    Commands {
        /// The destination, where more than one takes commands.
        #[arg(long)]
        destination: Option<String>,
    },
    /// List a server's roles with their ids, or make a mentionable opt-in
    /// role.
    Roles {
        /// The destination, where more than one takes commands.
        #[arg(long)]
        destination: Option<String>,
        /// The server, by id.
        #[arg(long)]
        guild: String,
        /// The name of a role to make.
        #[arg(long)]
        create: Option<String>,
    },
}

/// The commands that set Slack up to take commands.
#[derive(Debug, Subcommand)]
pub(crate) enum Slacking {
    /// Print the manifest of a Slack app that gives herald its commands.
    Manifest {
        /// The destination, where more than one takes commands.
        #[arg(long)]
        destination: Option<String>,
        /// The address herald is served at, where Slack posts commands to
        /// it.
        #[arg(long)]
        url: Option<String>,
    },
}

/// The commands that set Matrix up to take commands.
#[derive(Debug, Subcommand)]
pub(crate) enum Matrixing {
    /// Print the registration the homeserver installs, with two new tokens,
    /// once.
    Registration {
        /// The destination, where more than one takes commands.
        #[arg(long)]
        destination: Option<String>,
        /// The address herald is served at.
        #[arg(long)]
        url: String,
    },
}

/// A layer of support (ADR-0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Layer {
    /// The CLI, on GitHub Actions or anywhere else.
    Actions,
    /// `herald server`, in a container or on any host.
    Server,
    /// `herald lambda`, as an AWS Lambda function.
    Lambda,
    /// `herald server` as an Azure Functions custom handler.
    Azure,
    /// The wasm module on a Cloudflare Worker.
    Worker,
    /// The wasm module on Deno.
    Deno,
    /// The wasm module on Node or Bun.
    Node,
    /// The wasm module as a Vercel function.
    Vercel,
    /// The wasm module as a Netlify function.
    Netlify,
    /// The WASI component on a `wasi:http` host.
    Wasi,
    /// The WASI component on Spin.
    Spin,
    /// herald on Fastly Compute.
    Fastly,
}

impl Layer {
    /// Every layer, in ADR-0002's order: those with no endpoint, the
    /// serverless ones, and those that hold their endpoint open.
    pub(crate) const ALL: [Self; 12] = [
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

    /// What it serves.
    pub(crate) fn served(self) -> herald_flows::Layer<'static> {
        match self {
            Self::Actions => herald_flows::Layer {
                name: "actions",
                tier: Tier::Actions,
                stores: &STORES,
                endpoint: false,
                sessions: false,
                egress: Egress::Checked,
                rounds: Rounds::Workflow,
                round_secrets: &[],
                keyring: Memory::PerRequest,
            },
            Self::Server => herald_flows::Layer {
                name: "server",
                tier: Tier::Container,
                stores: &SERVED,
                endpoint: true,
                sessions: true,
                egress: Egress::Checked,
                rounds: Rounds::Scheduled,
                round_secrets: &[],
                keyring: Memory::Lasting,
            },
            Self::Lambda => herald_flows::Layer {
                name: "lambda",
                tier: Tier::Serverless,
                stores: &LAMBDA,
                endpoint: true,
                sessions: false,
                egress: Egress::Checked,
                rounds: Rounds::Platform(Cadence {
                    by: "the Rounds schedule in deploy/lambda/template.yaml",
                    poll: POLLED,
                    housekeep: POLLED,
                }),
                round_secrets: &[],
                keyring: Memory::PerRequest,
            },
            Self::Azure => herald_flows::Layer {
                name: "azure",
                tier: Tier::Serverless,
                stores: &SERVED,
                endpoint: true,
                sessions: false,
                egress: Egress::Checked,
                rounds: Rounds::Platform(Cadence {
                    by: "the poll and housekeep timer triggers in deploy/azure",
                    poll: POLLED,
                    housekeep: HOUSEKEPT,
                }),
                round_secrets: &[],
                keyring: Memory::Lasting,
            },
            Self::Worker => Hosting::Worker.layer(),
            Self::Deno => Hosting::Deno.layer(),
            Self::Node => Hosting::Node.layer(),
            Self::Vercel => Hosting::Vercel.layer(),
            Self::Netlify => Hosting::Netlify.layer(),
            Self::Wasi => Hosting::Wasi.layer(),
            Self::Spin => Hosting::Spin.layer(),
            Self::Fastly => Hosting::Fastly.layer(),
        }
    }

    /// Every layer's, in ADR-0002's order.
    pub(crate) fn every() -> [herald_flows::Layer<'static>; 12] {
        Self::ALL.map(Self::served)
    }
}

/// The commands that serve a config, each warning first of what it cannot.
#[derive(Debug, Subcommand)]
pub(crate) enum Serve {
    /// Announce the run's event to every route that takes it.
    Announce {
        /// Where the event comes from.
        #[arg(long, value_enum, required_unless_present = "event")]
        from: Option<From>,
        /// The release of the run's repository to announce, by its tag, in
        /// place of the run's own event.
        #[arg(long, conflicts_with = "event")]
        release: Option<String>,
        /// An app's event, as herald's event JSON, from this file, or from
        /// standard input where it is `-`.
        #[arg(long, requires = "app", conflicts_with = "from")]
        event: Option<String>,
        /// The app the event is of, as `[apps.<name>]` names it.
        #[arg(long, requires = "event")]
        app: Option<String>,
        /// Print every request a first announcement sends, with every secret
        /// redacted, and send nothing and keep no record.
        #[arg(long)]
        dry_run: bool,
    },
    /// Ask every source for what it has not seen, and announce it.
    Poll,
    /// Archive and lock every announcement thread whose time has come.
    Housekeep,
    /// Serve forge webhooks and herald's ActivityPub actors over HTTP, and
    /// poll and keep house on the config's schedule, until stopped.
    Server {
        /// The address and port to listen on, in place of `HERALD_LISTEN`
        /// and the config's.
        #[arg(long)]
        listen: Option<String>,
    },
    /// Serve as an AWS Lambda function on a custom runtime: answer its URL's
    /// requests, do the work they leave, and poll and keep house on its
    /// schedule.
    Lambda,
    /// Read the followers of herald's ActivityPub actors, and approve or
    /// reject a follow that waits.
    #[command(subcommand)]
    Followers(Following),
}

/// The commands on an ActivityPub actor's followers.
#[derive(Debug, Subcommand)]
pub(crate) enum Following {
    /// List who follows an actor, and whose follow waits for approval.
    List {
        /// The ActivityPub destination, as `[destinations.<name>]` names it.
        #[arg(long)]
        destination: String,
        /// The actor, as a route's `actor` names it.
        #[arg(long)]
        actor: String,
    },
    /// Approve a waiting follow: keep its follower and send it an `Accept`.
    Approve {
        /// The ActivityPub destination.
        #[arg(long)]
        destination: String,
        /// The actor followed.
        #[arg(long)]
        actor: String,
        /// The follower's actor id, as `list` prints it.
        #[arg(long)]
        follower: String,
    },
    /// Reject a waiting follow: forget its follower and send it a `Reject`.
    Reject {
        /// The ActivityPub destination.
        #[arg(long)]
        destination: String,
        /// The actor followed.
        #[arg(long)]
        actor: String,
        /// The follower's actor id, as `list` prints it.
        #[arg(long)]
        follower: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum From {
    /// The GitHub Actions run's own event.
    Github,
    /// The GitLab CI pipeline's tag.
    Gitlab,
    /// The Forgejo Actions run's own event.
    Forgejo,
}

impl From {
    pub(crate) const fn forge(self) -> Forge {
        match self {
            Self::Github => Forge::GitHub,
            Self::Gitlab => Forge::GitLab,
            Self::Forgejo => Forge::Forgejo,
        }
    }
}
