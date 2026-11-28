//! herald's command line (ADR-0009): its commands, their arguments, and the
//! layer each serves.

use std::sync::LazyLock;

use clap::builder::PossibleValue;
use clap::{Parser, Subcommand, ValueEnum};
use herald_sources_forge::Forge;

use herald_compose::Hosting;

use super::doctoring::DoctorArgs;

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
    /// The least grave line the log writes: debug, info, warn or error; in place
    /// of `HERALD_LOG_LEVEL` and the config's `[log] level`.
    #[arg(long, global = true, value_parser = herald_compose::log_level)]
    pub(crate) log_level: Option<herald_config::Level>,
    /// How the log writes each line: text or json; in place of
    /// `HERALD_LOG_FORMAT` and the config's `[log] format`.
    #[arg(long, global = true, value_parser = herald_compose::log_format)]
    pub(crate) log_format: Option<herald_config::LogFormat>,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Write a first herald.toml by answering a few questions, each also a
    /// flag with its default chosen, the store picked for the chat
    /// (ADR-0091), never over a config that exists; then offer one test
    /// message.
    Init(Box<crate::init::Answers>),
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
    /// Commands on herald's ActivityPub actors.
    #[command(subcommand)]
    Activitypub(Activitypub),
    #[command(flatten)]
    Read(Reading),
}

/// The commands on herald's ActivityPub actors.
#[derive(Debug, Subcommand)]
pub(crate) enum Activitypub {
    /// Write a new RSA key for the actors to FILE, as PKCS#8 PEM readable by
    /// its owner alone where the system has file modes (on Windows, FILE
    /// takes its folder's permissions), and print its public key; no part
    /// of the private key is printed. Store FILE's contents as the secret
    /// the destination's `key` names.
    Keygen {
        /// The file the private key is written to.
        file: std::path::PathBuf,
        /// How many bits the key's modulus holds.
        #[arg(long, value_enum, default_value_t = KeyBits::B2048)]
        bits: KeyBits,
        /// Replace FILE where it exists.
        #[arg(long)]
        force: bool,
    },
}

/// How many bits a new RSA key's modulus holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum KeyBits {
    /// 2048, as Mastodon's own keys are.
    #[value(name = "2048")]
    B2048,
    /// 3072.
    #[value(name = "3072")]
    B3072,
    /// 4096.
    #[value(name = "4096")]
    B4096,
}

impl From<KeyBits> for herald_crypto::keygen::Bits {
    fn from(bits: KeyBits) -> Self {
        match bits {
            KeyBits::B2048 => Self::Rsa2048,
            KeyBits::B3072 => Self::Rsa3072,
            KeyBits::B4096 => Self::Rsa4096,
        }
    }
}

/// What the operator says of a living post's possibly orphaned parts.
#[derive(Debug, Subcommand)]
pub(crate) enum Orphaning {
    /// List the possibly orphaned parts doctor names for a report, each
    /// route's with the command that clears it; with `--route`,
    /// `--deleted` and the `--seen` digest listed, say that route's parts
    /// were deleted by hand, so herald stops keeping them. Refuses where
    /// there are none, or where they changed since they were listed.
    Clear {
        /// The report, as doctor names it.
        report: String,
        /// Only this route's parts.
        #[arg(long)]
        route: Option<String>,
        /// Say each part listed for the route was deleted by hand, and
        /// stop keeping it.
        #[arg(long, requires_all = ["route", "seen"])]
        deleted: bool,
        /// The digest the listing printed for the route's parts: only
        /// those are cleared.
        #[arg(long, requires = "deleted")]
        seen: Option<String>,
    },
}

/// The commands that make tokens.
#[derive(Debug, Subcommand)]
pub(crate) enum Tokens {
    /// Print a new marker secret, once, to store as `HERALD_MARKER_SECRET`.
    Marker,
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
    Run(Running),
    /// Check the config, and name what it asks for that herald cannot serve.
    Doctor(DoctorArgs),
    /// Print the kind of store the config keeps its records in, as
    /// `[store] kind` writes it.
    Store,
    /// Print a view's open pull requests: whose turn each is, why, and how
    /// long it has waited.
    Queue {
        /// The view, as `[views.<name>]` names it; `queue`, every source's,
        /// where absent.
        view: Option<String>,
        /// How it is printed.
        #[arg(long, value_enum, default_value_t = QueueFormat::Table)]
        format: QueueFormat,
        /// Say this output is private, so the rows of private and internal
        /// repositories and a view's advisories are printed; without it they
        /// are withheld and counted, the rows printed where `[cli] private`
        /// says so (ADR-0067). A CI job's log may be public.
        #[arg(long)]
        private: bool,
    },
    /// List a rule's plans to re-run a flaky check, the default; or, with
    /// --apply, sign in at the forge and re-run each job --job names, once,
    /// with the source's write credential (ADR-0068).
    Rerun {
        /// The rule, as `[rules.<name>]` names it; every rule that plans
        /// re-runs where absent.
        rule: Option<String>,
        /// List the plans: nothing is re-run and no write credential is
        /// read.
        #[arg(long, conflicts_with = "apply")]
        plan: bool,
        /// Sign in at the forge and re-run each job --job names, where the
        /// plan still holds it on that head.
        #[arg(long, requires = "job")]
        apply: bool,
        /// A job to re-run, as the plan prints it: <job>@<head>.
        #[arg(long, value_name = "JOB@HEAD", requires = "apply")]
        job: Vec<String>,
        /// Say this output is private, so the plans of private and internal
        /// repositories are listed and applied; without it they are withheld
        /// and counted, unless `[cli] private` says so. A CI job's log may be
        /// public.
        #[arg(long)]
        private: bool,
    },
    /// Print each merge queue's entries read from its forge, what the queue
    /// does next with each and why; nothing is written and no write
    /// credential is read unless --apply is given (ADR-0087).
    MergeQueue {
        /// The queue, as `[merge_queues.<name>]` names it; every queue where
        /// absent.
        queue: Option<String>,
        /// Take the steps: hand each pull request due to the forge's queue
        /// with the queue's write credential, after reading it again and
        /// recording the step first. Without it nothing is written.
        #[arg(long)]
        apply: bool,
        /// Say this output is private, so the entries of private and
        /// internal repositories, and why a read failed, are printed;
        /// without it they are withheld, unless `[cli] private` says so. A
        /// CI job's log may be public.
        #[arg(long)]
        private: bool,
    },
    /// Write a part of the config by answering a question for each of its
    /// keys.
    #[command(subcommand)]
    Setup(SettingUp),
    /// Say what you did by hand about a living post's possibly orphaned
    /// parts, which doctor names (ADR-0067).
    #[command(subcommand)]
    Orphans(Orphaning),
    #[command(flatten)]
    Set(Setting),
    /// Trust another herald by the keys you confirm, list and check those
    /// trusted, or stop trusting one.
    #[command(subcommand)]
    Peer(Peering),
    /// Print the tables a `[presets.<name>]` table expands to, or write
    /// them into the config in its place (ADR-0092).
    #[command(subcommand)]
    Preset(Presetting),
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
        #[arg(long, value_enum, default_value_t = RunForge::Github)]
        from: RunForge,
        /// Render each route that groups against the file's release, or
        /// every sample release, as one group under this key.
        #[arg(long)]
        group: Option<String>,
    },
}

/// The commands on the heralds this one trusts.
#[derive(Debug, Subcommand)]
pub(crate) enum Peering {
    /// Read a herald's discovery document, show each key's Multikey and
    /// fingerprint, and pin the keys you confirm as [peers.<name>] in the
    /// config.
    Add {
        /// The herald's domain, or its address with any path its endpoints
        /// are under.
        url: String,
        /// The peer's name; its domain with each '.' as '-' where absent.
        #[arg(long)]
        name: Option<String>,
        /// A key to pin without asking, as a Multikey or a `sha256:`
        /// fingerprint its operator gave you; give it once for each key.
        #[arg(long)]
        key: Vec<String>,
        /// Print the table rather than write it into the config.
        #[arg(long)]
        print: bool,
        /// Pin the keys a peer already named lists now.
        #[arg(long)]
        update: bool,
    },
    /// List each peer with its pinned keys.
    List,
    /// Read each peer's discovery document, checked against its pinned
    /// keys, and name each key set that changed.
    Test {
        /// Only this peer.
        name: Option<String>,
    },
    /// Show what each peer asked of this herald and how it was answered,
    /// newest first, from the audit the store keeps of each day
    /// (ADR-0054); never a request's body.
    Log {
        /// Only this peer; "-" for requests signed by keys no peer holds.
        name: Option<String>,
        /// Only requests since this day, as YYYY-MM-DD, this time, in
        /// RFC 3339, or this long ago, such as "7d".
        #[arg(long)]
        since: Option<String>,
        /// The most requests shown.
        #[arg(long, default_value_t = LOG_LIMIT)]
        limit: usize,
        /// How they are printed.
        #[arg(long, value_enum, default_value_t = Shown::Text)]
        format: Shown,
    },
    /// Stop trusting a peer, or one of its keys: taken from the config and
    /// added to [federation] revoked, so it is never pinned again.
    Revoke {
        /// The peer, by name.
        name: String,
        /// Only this key, as a Multikey or a `sha256:` fingerprint; the peer
        /// is kept while it pins another.
        #[arg(long)]
        key: Option<String>,
    },
}

/// How many requests `herald peer log` shows where `--limit` is absent.
pub(crate) const LOG_LIMIT: usize = 1000;

/// How a listing is printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Shown {
    /// One line each.
    Text,
    /// One JSON array.
    Json,
}

/// The parts of the config a wizard writes.
#[derive(Debug, Subcommand)]
pub(crate) enum SettingUp {
    /// Write a `[merge_queues.<name>]` table, or edit one, by answering a
    /// question for each key, each with its default chosen; it starts no
    /// queue (ADR-0087).
    MergeQueue(Box<crate::queue_setup::Answers>),
}

/// What `herald preset` does with a `[presets.<name>]` table.
#[derive(Debug, Subcommand)]
pub(crate) enum Presetting {
    /// Print the tables it expands to, as herald.toml would write them.
    Show {
        /// The table, as `[presets.<name>]` names it.
        name: String,
    },
    /// Write the tables it expands to into the config in its place, once
    /// confirmed and the config they make read back whole.
    Eject {
        /// The table, as `[presets.<name>]` names it.
        name: String,
        /// Write without asking.
        #[arg(long)]
        yes: bool,
        /// Print the config as it would be written, and write nothing.
        #[arg(long)]
        print: bool,
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
    /// Register a webhook on each member of a GitLab owner source whose
    /// `member_hooks` asks for one (ADR-0041).
    Hooks {
        /// The source, by name.
        name: String,
        /// The address herald is served at; `[server] url` where absent.
        #[arg(long)]
        url: Option<String>,
    },
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

/// A host, as `--layer` names a layer of support (ADR-0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layer(pub(crate) Hosting);

/// Every layer, in ADR-0002's order.
static LAYERS: LazyLock<[Layer; Hosting::ALL.len()]> = LazyLock::new(|| Hosting::ALL.map(Layer));

impl ValueEnum for Layer {
    fn value_variants<'a>() -> &'a [Self] {
        &*LAYERS
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.0.name()).help(self.0.description()))
    }
}

/// The commands that run against the config's routes, sources and store:
/// announcing, polling, housekeeping, serving and reading an actor's
/// followers. Each but `followers` warns first of what the layer it runs on
/// cannot serve.
#[derive(Debug, Subcommand)]
pub(crate) enum Running {
    /// Announce the run's event to every route that takes it.
    Announce {
        /// Where the event comes from.
        #[arg(long, value_enum, required_unless_present = "event")]
        from: Option<RunForge>,
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
        /// Say this output is private, so an event of a private or internal
        /// repository is named by its version or title; without it, or
        /// `[cli] private`, it is named "(private)" (ADR-0080). A CI job's
        /// log may be public.
        #[arg(long)]
        private: bool,
    },
    /// Ask every source for what it has not seen, and announce it.
    Poll {
        /// Say this output is private, so an event of a private or internal
        /// repository is named by its version or title; without it, or
        /// `[cli] private`, it is named "(private)" (ADR-0080). A CI job's
        /// log may be public.
        #[arg(long)]
        private: bool,
    },
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
    /// Withdraw a release's posts from a route, or from every route,
    /// whatever its `withdraw` says: each message deleted, its place in a
    /// group taken out, its feed entry replaced by a tombstone.
    Withdraw {
        /// The route and the release's identity, or the identity alone with
        /// --every-route.
        #[arg(num_args = 1..=2, value_names = ["ROUTE", "IDENTITY"], required = true)]
        names: Vec<String>,
        /// Every route, in place of one named.
        #[arg(long)]
        every_route: bool,
        /// Print what each route would withdraw, and send nothing and keep
        /// no record.
        #[arg(long)]
        dry_run: bool,
    },
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

/// The forge whose CI run's own event is read.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum RunForge {
    /// The GitHub Actions run's own event.
    Github,
    /// The GitLab CI pipeline's tag.
    Gitlab,
    /// The Forgejo Actions run's own event.
    Forgejo,
}

impl RunForge {
    pub(crate) const fn forge(self) -> Forge {
        match self {
            Self::Github => Forge::GitHub,
            Self::Gitlab => Forge::GitLab,
            Self::Forgejo => Forge::Forgejo,
        }
    }
}

/// How `herald queue` prints a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum QueueFormat {
    /// A table to read.
    Table,
    /// JSON, with the keys a report's template reads as `view`.
    Json,
}
