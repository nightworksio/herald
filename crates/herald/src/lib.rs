//! herald's CLI (ADR-0009): `herald announce --from github`, `herald poll`
//! and `herald housekeep` on GitHub Actions, and `herald doctor` anywhere.
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
use herald_core::{Chore, Event, Schedule, SourceName, Step};
use herald_flows::{
    Announced, Chored, Connected, Findings, Gap, Kept, Outcome, Ports, Remembered, Stop, Sweep,
    Swept, announce, doctor, housekeep, poll, prepare, unserved,
};
use herald_native::{EnvSecrets, RingSigner, SystemClock};
use herald_ports::{Http, HttpError, Request, Response, Secrets, Store as Keeps};
use herald_sources_forge::GitHub;
use herald_store_git_branch::{GitBranch, GitBranchStore, StoreOptions};

/// Every route announced, already announced, or held by another run; every
/// source polled and its cursor moved on; or every chore that fell due done
/// or dropped.
pub const DONE: u8 = 0;
/// A route stopped part-way, a source waits for the next poll, a chore
/// waits for the next run, or the run could not start.
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
    #[command(flatten)]
    Serve(Serve),
    /// Check the config, and name what it asks for that herald cannot serve.
    Doctor,
}

/// The commands that serve a config, each warning first of what it cannot.
#[derive(Debug, Subcommand)]
enum Serve {
    /// Announce the run's event to every route that takes it.
    Announce {
        /// Where the event comes from.
        #[arg(long, value_enum)]
        from: From,
    },
    /// Ask every source for what it has not seen, and announce it.
    Poll,
    /// Archive and lock every announcement thread whose time has come.
    Housekeep,
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
            say(err, &message);
            return MISTAKEN;
        }
    };
    let findings = doctor(&config, &STORES);
    let Command::Serve(serve) = cli.command else {
        return checked(&cli.config, &findings, out);
    };
    warned(&findings.gaps, vars, out);
    let ran = match serve {
        Serve::Announce { from: From::Github } => github(&config, vars, connect, out).await,
        Serve::Poll => polling(&config, vars, connect, out).await,
        Serve::Housekeep => housekeeping(&config, vars, connect, out).await,
    };
    ran.unwrap_or_else(|message| {
        say(err, &message);
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

fn checked(path: &str, findings: &Findings, out: &mut dyn Write) -> u8 {
    say(
        out,
        &format!("{path} reads, its names resolve and its templates render."),
    );
    for note in &findings.notes {
        say(out, note);
    }
    for gap in &findings.gaps {
        say(out, &gapped(gap));
    }
    if findings.gaps.is_empty() {
        DONE
    } else {
        STOPPED
    }
}

fn gapped(gap: &Gap) -> String {
    format!(
        "{}: {} cannot be served: {}.",
        gap.key, gap.feature, gap.why
    )
}

/// Each gap as a warning, annotated on GitHub Actions.
fn warned(gaps: &[Gap], vars: &BTreeMap<String, String>, out: &mut dyn Write) {
    let actions = vars
        .get("GITHUB_ACTIONS")
        .is_some_and(|value| value == "true");
    for gap in gaps {
        let line = gapped(gap);
        if actions {
            say(
                out,
                &format!("::warning title=herald doctor::{}", escaped(&line)),
            );
        } else {
            say(out, &format!("warning: {line}"));
        }
    }
}

/// Writes `line` as one line: a line break inside it, from a name or an
/// answer herald was handed, would otherwise start a line of its own, which
/// GitHub Actions could read as a workflow command.
fn say(to: &mut dyn Write, line: &str) {
    let _ = writeln!(to, "{}", line.replace(['\r', '\n'], " "));
}

/// `message` as one workflow command's message: a line break would start
/// another command.
fn escaped(message: &str) -> String {
    message
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
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
        say(
            out,
            &format!("The run's event is {event_name}, not a release; nothing to announce."),
        );
        return Ok(DONE);
    }
    let path = variable(vars, "GITHUB_EVENT_PATH")?;
    let payload =
        std::fs::read(path).map_err(|error| format!("{path} could not be read: {error}"))?;
    let Some(event) = GitHub::event(&payload).map_err(|error| error.to_string())? else {
        say(
            out,
            "The release is not published, or is a draft; nothing to announce.",
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
    let (records, connected) = reached(config, vars, connect).await?;
    let run = run_name(vars);
    let announcing = Announcing {
        config,
        connected: &connected,
        sources: &sources,
        event: &event,
        run: &run,
    };
    Ok(match &records {
        Records::Branch(store) => announcing.to(store, out).await,
        Records::Posts => announcing.to(&connected.marked(RingSigner), out).await,
    })
}

/// An event, and the routes and sources it is announced through.
struct Announcing<'a> {
    config: &'a Config,
    connected: &'a Connected<Connection>,
    sources: &'a [&'a SourceName],
    event: &'a Event,
    run: &'a str,
}

impl Announcing<'_> {
    async fn to<S: Keeps>(&self, store: &S, out: &mut dyn Write) -> u8 {
        let ports = Ports {
            store,
            clock: &SystemClock,
            signer: &RingSigner,
        };
        let mut code = DONE;
        for source in self.sources {
            let announced = announce(
                self.config,
                self.connected,
                &ports,
                source,
                self.event,
                self.run,
            )
            .await;
            for announced in announced {
                if matches!(announced.outcome, Outcome::Stopped { .. }) {
                    code = STOPPED;
                }
                say(out, &report(&announced));
            }
        }
        code
    }
}

/// The name this run takes its leases under.
fn run_name(vars: &BTreeMap<String, String>) -> String {
    match (vars.get("GITHUB_RUN_ID"), vars.get("GITHUB_RUN_ATTEMPT")) {
        (Some(id), Some(attempt)) => format!("github/{id}/{attempt}"),
        _ => "local".to_owned(),
    }
}

async fn polling(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (records, connected) = reached(config, vars, connect).await?;
    let run = run_name(vars);
    let swept = match &records {
        Records::Branch(store) => swept(config, &connected, store, &run).await,
        Records::Posts => swept(config, &connected, &connected.marked(RingSigner), &run).await,
    };
    if swept.is_empty() {
        say(out, "The config names no source to poll.");
    }
    let mut code = DONE;
    for swept in &swept {
        if let Sweep::Stopped(_)
        | Sweep::Polled {
            cursor: Remembered::Kept | Remembered::Failed(_),
            ..
        } = swept.outcome
        {
            code = STOPPED;
        }
        for line in polled(swept) {
            say(out, &line);
        }
    }
    Ok(code)
}

async fn swept<S: Keeps>(
    config: &Config,
    connected: &Connected<Connection>,
    store: &S,
    run: &str,
) -> Vec<Swept> {
    let ports = Ports {
        store,
        clock: &SystemClock,
        signer: &RingSigner,
    };
    poll(config, connected, &ports, run).await
}

fn polled(swept: &Swept) -> Vec<String> {
    let source = swept.source.as_str();
    match &swept.outcome {
        Sweep::Unchanged => vec![format!("{source}: unchanged since the last poll")],
        Sweep::Stopped(stop) => vec![format!(
            "{source}: stopped: {}. The next poll asks again.",
            why(stop)
        )],
        Sweep::Polled {
            first,
            found,
            announced,
            cursor,
        } => {
            let new = announced.len();
            let head = if *first {
                format!("{source}: first poll found {found}; {new} announced, as backfill asks")
            } else {
                format!("{source}: {found} found, {new} new")
            };
            let tail = match cursor {
                Remembered::Advanced => String::new(),
                Remembered::Kept => {
                    "; a route stopped, so the next poll offers them again".to_owned()
                }
                Remembered::Failed(stop) => format!(
                    "; the cursor stays where it was: {}. The next poll offers them again",
                    why(stop)
                ),
            };
            let mut lines = vec![format!("{head}{tail}")];
            for fresh in announced {
                let event = fresh.event.version().unwrap_or(fresh.event.title());
                lines.extend(
                    fresh
                        .announced
                        .iter()
                        .map(|announced| format!("{source} {event}: {}", report(announced))),
                );
            }
            lines
        }
    }
}

/// Where the run's records are kept.
enum Records {
    /// On a git branch.
    Branch(GitBranchStore<Connection>),
    /// In the routes' own posts.
    Posts,
}

/// The store kinds this herald keeps records in.
const STORES: [&str; 2] = ["git-branch", "markers"];

/// The run's records, and every route and source connected.
async fn reached(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
) -> Result<(Records, Connected<Connection>), String> {
    let secrets = EnvSecrets::from_vars(vars.clone());
    let records = match &config.store {
        Store::GitBranch(options) => {
            Records::Branch(branch(options, vars, &secrets, connect).await?)
        }
        Store::Markers => Records::Posts,
        _ => return Err(unserved(&STORES)),
    };
    let prepared = prepare(config, &secrets, &RingSigner)
        .await
        .map_err(|error| error.to_string())?;
    Ok((records, prepared.connect(connect)?))
}

async fn branch(
    options: &StoreOptions,
    vars: &BTreeMap<String, String>,
    secrets: &EnvSecrets,
    connect: Connect<'_>,
) -> Result<GitBranchStore<Connection>, String> {
    let repository = variable(vars, "GITHUB_REPOSITORY")?;
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
    Ok(GitBranchStore::new(branch.clone(), connect(branch.api())?))
}

async fn housekeeping(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (records, connected) = reached(config, vars, connect).await?;
    let kept = match &records {
        Records::Branch(store) => housekeep(&connected, store, &SystemClock).await,
        Records::Posts => housekeep(&connected, &connected.marked(RingSigner), &SystemClock).await,
    }
    .map_err(|stop| why(&stop))?;
    if kept.is_empty() {
        say(out, "No chore is due.");
    }
    let mut code = DONE;
    for kept in &kept {
        if matches!(kept.outcome, Chored::Later(_)) {
            code = STOPPED;
        }
        say(out, &chored(kept));
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
        | Stop::Marker(reason)
        | Stop::Source(reason) => reason.clone(),
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
