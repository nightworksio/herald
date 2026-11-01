//! herald's CLI (ADR-0009): `herald announce --from github` and
//! `herald housekeep` on GitHub Actions, and `herald doctor` anywhere.
//!
//! [`run`] is the whole CLI, handed its arguments, its environment and the
//! way to reach each base URL, so it runs the same under test as on a runner.

use std::collections::BTreeMap;
use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};
use herald_config::{Config, Source, Store, load};
use herald_core::{Chore, Schedule, SourceName, Step};
use herald_flows::{
    Announced, Chored, Connected, Kept, Outcome, Ports, Stop, announce, housekeep, prepare,
};
use herald_native::{EnvSecrets, RingSigner, SystemClock};
use herald_ports::{Http, HttpError, Request, Response, Secrets};
use herald_sources_forge::GitHub;
use herald_store_git_branch::{GitBranch, GitBranchStore};

/// Every route announced, already announced, or held by another run; or
/// every chore that fell due done or dropped.
pub const DONE: u8 = 0;
/// A route stopped part-way, a chore waits for the next run, or the run
/// could not start.
pub const STOPPED: u8 = 1;
/// The config has mistakes.
pub const MISTAKEN: u8 = 2;

/// The secret a git-branch store's token is read from where the config names
/// none.
const GITHUB_TOKEN: &str = "GITHUB_TOKEN";

type Sending<'a> = Pin<Box<dyn Future<Output = Result<Response, HttpError>> + Send + 'a>>;

trait Sends: Send + Sync {
    fn sending(&self, request: Request) -> Sending<'_>;
}

impl<H: Http + Send> Sends for H {
    fn sending(&self, request: Request) -> Sending<'_> {
        Box::pin(self.send(request))
    }
}

/// One base URL's `Http`, whatever reaches it.
#[derive(Clone)]
pub struct Connection(Arc<dyn Sends>);

impl Connection {
    /// The connection `http` makes.
    pub fn new(http: impl Http + Send + 'static) -> Self {
        Self(Arc::new(http))
    }
}

impl Http for Connection {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        self.0.sending(request)
    }
}

/// What makes the connection to a base URL.
pub type Connect<'a> = &'a dyn Fn(&str) -> Result<Connection, String>;

#[derive(Debug, Parser)]
#[command(
    name = "herald",
    version,
    about = "Announce what happened to the places people gather."
)]
struct Cli {
    /// The config to read.
    #[arg(long, global = true, default_value = "herald.toml")]
    config: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Announce the run's event to every route that takes it.
    Announce {
        /// Where the event comes from.
        #[arg(long, value_enum)]
        from: From,
    },
    /// Archive and lock every announcement thread whose time has come.
    Housekeep,
    /// Check the config, and name what it asks for that herald cannot serve.
    Doctor,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum From {
    /// The GitHub Actions run's own event.
    Github,
}

/// The CLI, run with `args`, in an environment of `vars`, reaching each base
/// URL through what `connect` makes for it, and writing to `out` and `err`.
/// It answers the process's exit code.
pub async fn run(
    args: &[String],
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let rendered = error.render();
            let _ = if error.use_stderr() {
                write!(err, "{rendered}")
            } else {
                write!(out, "{rendered}")
            };
            return u8::try_from(error.exit_code()).unwrap_or(MISTAKEN);
        }
    };
    let config = match read(&cli.config) {
        Ok(config) => config,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return MISTAKEN;
        }
    };
    let ran = match cli.command {
        Command::Doctor => return doctor(&cli.config, &config, out),
        Command::Announce { from: From::Github } => github(&config, vars, connect, out).await,
        Command::Housekeep => housekeeping(&config, vars, connect, out).await,
    };
    ran.unwrap_or_else(|message| {
        let _ = writeln!(err, "{message}");
        STOPPED
    })
}

fn read(path: &str) -> Result<Config, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("{path} could not be read: {error}"))?;
    let directory = Path::new(path).parent().unwrap_or(Path::new(""));
    let files = |file: &str| {
        std::fs::read_to_string(directory.join(file)).map_err(|error| error.to_string())
    };
    load(path, &text, &files).map_err(|errors| errors.to_string())
}

fn doctor(path: &str, config: &Config, out: &mut dyn Write) -> u8 {
    let _ = writeln!(
        out,
        "{path} reads, its names resolve and its templates render."
    );
    if config.marker_key.is_none() {
        let _ = writeln!(
            out,
            "Each post's markers are keyed by its destination's credential, as no [markers] key is set. Rotating that credential forgets every earlier marker, so an announcement made before the rotation can be posted again on a store that has only markers. Set [markers] key = {{ secret = \"HERALD_MARKER_SECRET\" }} to keep them."
        );
    }
    if !matches!(config.store, Store::GitBranch(_)) {
        let _ = writeln!(
            out,
            "This herald keeps records only on a git branch; set [store] kind = \"git-branch\"."
        );
        return STOPPED;
    }
    DONE
}

fn variable<'a>(vars: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, String> {
    vars.get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is not set; this command runs in GitHub Actions"))
}

async fn github(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let event_name = variable(vars, "GITHUB_EVENT_NAME")?;
    if event_name != "release" {
        let _ = writeln!(
            out,
            "The run's event is {event_name}, not a release; nothing to announce."
        );
        return Ok(DONE);
    }
    let path = variable(vars, "GITHUB_EVENT_PATH")?;
    let payload =
        std::fs::read(path).map_err(|error| format!("{path} could not be read: {error}"))?;
    let Some(event) = GitHub::event(&payload).map_err(|error| error.to_string())? else {
        let _ = writeln!(
            out,
            "The release is not published, or is a draft; nothing to announce."
        );
        return Ok(DONE);
    };
    let repository = variable(vars, "GITHUB_REPOSITORY")?;
    let sources: Vec<&SourceName> = config
        .sources
        .iter()
        .filter(|(_, source)| match source {
            Source::GitHub(options) => options.repository.eq_ignore_ascii_case(repository),
        })
        .map(|(name, _)| name)
        .collect();
    if sources.is_empty() {
        return Err(format!(
            "no source in the config names {repository}; add [sources.<name>] with kind = \"github\" and repository = \"{repository}\""
        ));
    }
    let (store, connected) = reached(config, vars, connect).await?;
    let ports = Ports {
        store: &store,
        clock: &SystemClock,
        signer: &RingSigner,
    };
    let run = match (vars.get("GITHUB_RUN_ID"), vars.get("GITHUB_RUN_ATTEMPT")) {
        (Some(id), Some(attempt)) => format!("github/{id}/{attempt}"),
        _ => "local".to_owned(),
    };
    let mut code = DONE;
    for source in sources {
        for announced in announce(config, &connected, &ports, source, &event, &run).await {
            if matches!(announced.outcome, Outcome::Stopped { .. }) {
                code = STOPPED;
            }
            let _ = writeln!(out, "{}", report(&announced));
        }
    }
    Ok(code)
}

/// The run's store, and every route's destination connected.
async fn reached(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
) -> Result<(GitBranchStore<Connection>, Connected<Connection>), String> {
    let repository = variable(vars, "GITHUB_REPOSITORY")?;
    let Store::GitBranch(options) = &config.store else {
        return Err(
            "this herald keeps records only on a git branch; set [store] kind = \"git-branch\""
                .to_owned(),
        );
    };
    let secrets = EnvSecrets::from_vars(vars.clone());
    let token_name = options
        .token
        .as_ref()
        .map_or(GITHUB_TOKEN, |token| token.secret.as_str());
    let token = secrets.secret(token_name).await.map_err(|_| {
        format!("{token_name} is not set; the git-branch store needs a token that may push")
    })?;
    let branch = GitBranch::new(
        options.repository.as_deref().unwrap_or(repository),
        token,
        options.branch.as_deref(),
        options.api.as_deref(),
    );
    let store = GitBranchStore::new(branch.clone(), connect(branch.api())?);
    let prepared = prepare(config, &secrets, &RingSigner)
        .await
        .map_err(|error| error.to_string())?;
    Ok((store, prepared.connect(connect)?))
}

async fn housekeeping(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (store, connected) = reached(config, vars, connect).await?;
    let kept = housekeep(&connected, &store, &SystemClock)
        .await
        .map_err(|stop| why(&stop))?;
    if kept.is_empty() {
        let _ = writeln!(out, "No chore is due.");
    }
    let mut code = DONE;
    for kept in &kept {
        if matches!(kept.outcome, Chored::Later(_)) {
            code = STOPPED;
        }
        let _ = writeln!(out, "{}", chored(kept));
    }
    Ok(code)
}

fn chored(kept: &Kept) -> String {
    let Schedule {
        route,
        thread,
        chore,
        ..
    } = &kept.schedule;
    let route = route.as_str();
    let done = match chore {
        Chore::Archive => "archived",
        Chore::Lock => "locked",
    };
    match &kept.outcome {
        Chored::Done => format!("{route}: thread {thread} {done}"),
        Chored::Dropped(reason) => {
            format!("{route}: thread {thread} not {done}, and not tried again: {reason}")
        }
        Chored::Later(stop) => format!(
            "{route}: thread {thread} not {done} yet: {}. The next run tries again.",
            why(stop)
        ),
    }
}

fn why(reason: &Stop) -> String {
    match reason {
        Stop::RetryAfter(after) => format!("the destination asked to wait {after:#}"),
        Stop::Refused(reason)
        | Stop::Unreachable(reason)
        | Stop::Store(reason)
        | Stop::Template(reason)
        | Stop::Marker(reason) => reason.clone(),
    }
}

fn step(step: &Step) -> String {
    match step {
        Step::Post => "post".to_owned(),
        Step::Crosspost => "crosspost".to_owned(),
        Step::OpenThread => "thread".to_owned(),
        Step::Chunk(index) => format!("chunk {index}"),
        Step::Pin => "pin".to_owned(),
        Step::Unpin => "unpin".to_owned(),
        Step::Deliver(inbox) => format!("delivery to {inbox}"),
    }
}

fn steps(steps: &[Step]) -> String {
    steps.iter().map(step).collect::<Vec<_>>().join(", ")
}

fn report(announced: &Announced) -> String {
    let route = announced.route.as_str();
    match &announced.outcome {
        Outcome::Sent { steps: sent } => format!("{route}: announced ({})", steps(sent)),
        Outcome::AlreadyAnnounced => format!("{route}: already announced; nothing sent"),
        Outcome::Held { by, until } => {
            format!("{route}: run {by} holds it until {until}; nothing sent by this run")
        }
        Outcome::Stopped {
            landed,
            remaining,
            reason,
        } => {
            format!(
                "{route}: stopped: {}. Landed: {}. The next run sends what remains: {}.",
                why(reason),
                if landed.is_empty() {
                    "nothing".to_owned()
                } else {
                    steps(landed)
                },
                if remaining.is_empty() {
                    "nothing".to_owned()
                } else {
                    steps(remaining)
                },
            )
        }
    }
}

#[cfg(test)]
mod tests;
