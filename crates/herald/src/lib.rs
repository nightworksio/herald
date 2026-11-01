//! herald's CLI (ADR-0009): `herald announce --from github`, `--from gitlab`
//! or `--from forgejo` in a forge's CI, `herald poll` and `herald housekeep`
//! on a schedule, and `herald doctor` anywhere.
//!
//! [`run`] is the whole CLI, handed its arguments, its environment and the
//! way to reach each base URL, so it runs the same under test as on a runner.

use std::collections::BTreeMap;
use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use clap::{Parser, Subcommand, ValueEnum};
use herald_config::{Config, Source, Store, load, samples};
use herald_core::{Chore, Event, Kind, RouteId, Schedule, SourceName, Step};
use herald_flows::{
    Announced, Chored, Connected, Findings, Gap, Kept, Outcome, Ports, Remembered, Stop, Sweep,
    Swept, announce, doctor, housekeep, poll, prepare, preview, unserved,
};
use herald_native::{EnvSecrets, RingSigner, SystemClock, random_secret};
use herald_ports::{Http, HttpError, Request, Response, Secrets, Store as Keeps};
use herald_records::Memory;
use herald_sources_forge::{Forge, Origin, Repository, SourceOptions, Token};
use herald_store_actions_cache::{ActionsCache, ActionsCacheStore, StoreOptions as CacheOptions};
use herald_store_git_branch::{GitBranch, GitBranchStore, StoreOptions as BranchOptions};
use herald_store_repo_variables::{
    RepoVariables, RepoVariablesStore, StoreOptions as VariablesOptions,
};

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
/// Where the Actions cache's results service is, as herald's Action hands
/// it on.
const RESULTS: &str = "ACTIONS_RESULTS_URL";
/// The run's token for the Actions cache, as herald's Action hands it on.
const RUNTIME_TOKEN: &str = "ACTIONS_RUNTIME_TOKEN";

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
pub type Connect<'a> = &'a (dyn Fn(&str) -> Result<Connection, String> + Send + Sync);

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
    /// Print a new marker secret, once, to store as `HERALD_MARKER_SECRET`.
    Init,
    #[command(flatten)]
    Read(Reading),
}

/// The commands that read the config.
#[derive(Debug, Subcommand)]
enum Reading {
    #[command(flatten)]
    Serve(Serve),
    /// Check the config, and name what it asks for that herald cannot serve.
    Doctor {
        /// The layer to check the config for.
        #[arg(long, value_enum, default_value_t = Layer::Actions)]
        layer: Layer,
    },
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

/// A layer of support (ADR-0002).
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Layer {
    /// The CLI, on GitHub Actions or anywhere else.
    Actions,
}

impl Layer {
    /// The store kinds it keeps records in.
    fn stores(self) -> &'static [&'static str] {
        match self {
            Self::Actions => &STORES,
        }
    }
}

/// The commands that serve a config, each warning first of what it cannot.
#[derive(Debug, Subcommand)]
enum Serve {
    /// Announce the run's event to every route that takes it.
    Announce {
        /// Where the event comes from.
        #[arg(long, value_enum)]
        from: From,
        /// The release of the run's repository to announce, by its tag, in
        /// place of the run's own event.
        #[arg(long)]
        release: Option<String>,
        /// Print every request a first announcement sends, with every secret
        /// redacted, and send nothing and keep no record.
        #[arg(long)]
        dry_run: bool,
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
    /// The GitLab CI pipeline's tag.
    Gitlab,
    /// The Forgejo Actions run's own event.
    Forgejo,
}

impl From {
    const fn forge(self) -> Forge {
        match self {
            Self::Github => Forge::GitHub,
            Self::Gitlab => Forge::GitLab,
            Self::Forgejo => Forge::Forgejo,
        }
    }
}

/// The variables a forge's CI names a run's repository, API and token in.
struct Ci {
    /// The repository's path.
    repository: &'static str,
    /// The base URL of the forge's API.
    api: &'static str,
    /// The run's own token.
    token: &'static str,
}

impl Ci {
    const fn of(forge: Forge) -> Self {
        match forge {
            Forge::GitHub | Forge::Forgejo => Self {
                repository: "GITHUB_REPOSITORY",
                api: "GITHUB_API_URL",
                token: GITHUB_TOKEN,
            },
            Forge::GitLab => Self {
                repository: "CI_PROJECT_PATH",
                api: "CI_API_V4_URL",
                token: "CI_JOB_TOKEN",
            },
        }
    }
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
    let reading = match cli.command {
        Command::Init => return init(random_secret(), out, err),
        Command::Read(reading) => reading,
    };
    let config = match read(&cli.config) {
        Ok(config) => config,
        Err(message) => {
            say(err, &message);
            return MISTAKEN;
        }
    };
    let serve = match reading {
        Reading::Doctor { layer } => {
            return checked(&cli.config, &doctor(&config, layer.stores()), out);
        }
        Reading::Preview { route, event, from } => {
            let event = event.as_deref().map(|path| (from.forge(), path));
            return previewing(&config, route.as_deref(), event, out, err);
        }
        Reading::Serve(serve) => serve,
    };
    masked(&config, vars, out);
    warned(&doctor(&config, Layer::Actions.stores()).gaps, vars, out);
    let ran = match serve {
        Serve::Announce {
            from,
            release,
            dry_run,
        } => {
            let announcing = Announce {
                forge: from.forge(),
                release: release.as_deref(),
                dry_run,
            };
            announcing_from(&config, vars, connect, announcing, out).await
        }
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

/// Whether the run is on GitHub Actions.
fn on_actions(vars: &BTreeMap<String, String>) -> bool {
    vars.get("GITHUB_ACTIONS")
        .is_some_and(|value| value == "true")
}

/// On GitHub Actions, every secret the run may resolve masked before
/// anything else is printed, each of its lines on its own as well
/// (ADR-0007).
fn masked(config: &Config, vars: &BTreeMap<String, String>, out: &mut dyn Write) {
    if !on_actions(vars) {
        return;
    }
    let mut names = config.secrets();
    names.extend([GITHUB_TOKEN, RUNTIME_TOKEN]);
    for value in names.iter().filter_map(|name| vars.get(*name)) {
        let lines = value.lines().filter(|line| *line != value.as_str());
        for hidden in std::iter::once(value.as_str()).chain(lines) {
            if !hidden.trim().is_empty() {
                say(out, &format!("::add-mask::{}", escaped(hidden)));
            }
        }
    }
}

/// Each gap as a warning, annotated on GitHub Actions.
fn warned(gaps: &[Gap], vars: &BTreeMap<String, String>, out: &mut dyn Write) {
    let actions = on_actions(vars);
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
        .ok_or_else(|| format!("{name} is not set; herald reads it from the CI run"))
}

/// How an announcement is asked for.
#[derive(Debug, Clone, Copy)]
struct Announce<'a> {
    /// The forge whose CI the run is in.
    forge: Forge,
    /// The release to announce by its tag, in place of the run's event.
    release: Option<&'a str>,
    /// Whether to print the requests and send nothing.
    dry_run: bool,
}

async fn announcing_from(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    asked: Announce<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let forge = asked.forge;
    let ci = Ci::of(forge);
    let repository = variable(vars, ci.repository)?;
    let named: Vec<(&SourceName, &SourceOptions)> = config
        .sources
        .iter()
        .filter_map(|(name, source)| match source {
            Source::Forge(kind, options)
                if *kind == forge && options.repository.eq_ignore_ascii_case(repository) =>
            {
                Some((name, options))
            }
            _ => None,
        })
        .collect();
    let Some((_, named_options)) = named.first() else {
        return Err(format!(
            "no source in the config names {repository}; add [sources.<name>] with kind = \"{}\" and repository = \"{repository}\"",
            forge.kind()
        ));
    };
    let options = &SourceOptions {
        api: named_options
            .api
            .clone()
            .or_else(|| vars.get(ci.api).filter(|api| !api.is_empty()).cloned()),
        ..(*named_options).clone()
    };
    let tag = match forge {
        Forge::GitLab => asked.release.or_else(|| {
            vars.get("CI_COMMIT_TAG")
                .map(String::as_str)
                .filter(|tag| !tag.is_empty())
        }),
        Forge::GitHub | Forge::Forgejo => asked.release,
    };
    let event = match (forge, tag) {
        (Forge::GitLab, None) => {
            say(out, "The pipeline is not for a tag; nothing to announce.");
            None
        }
        (Forge::GitLab, Some(tag)) => {
            let project = project(vars)?;
            tagged(options, forge, tag, Some(project), vars, connect, out).await?
        }
        (_, Some(tag)) => tagged(options, forge, tag, None, vars, connect, out).await?,
        (_, None) => evented(forge, vars, out)?,
    };
    let Some(event) = event else {
        return Ok(DONE);
    };
    let sources: Vec<&SourceName> = named.iter().map(|(name, _)| *name).collect();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let (records, connected) = if asked.dry_run {
        dry(config, vars, &sent).await?
    } else {
        reached(config, vars, connect).await?
    };
    let run = run_name(vars);
    let announcing = Announcing {
        config,
        connected: &connected,
        sources: &sources,
        event: &event,
        run: &run,
    };
    let announced = records.serve(&connected, &announcing).await;
    if asked.dry_run {
        say(
            out,
            "A dry run sends nothing and keeps no record: these are the requests a first announcement sends.",
        );
        for request in sent.lock().unwrap_or_else(PoisonError::into_inner).iter() {
            say(out, "");
            for line in request.lines() {
                say(out, line);
            }
        }
        say(out, "");
    }
    let mut code = DONE;
    for announced in announced {
        if matches!(announced.outcome, Outcome::Stopped { .. }) {
            code = STOPPED;
        }
        say(out, &report(&announced));
    }
    Ok(code)
}

/// An `Http` that keeps each request it is handed, sends none, and answers
/// each with a success, for a dry run.
struct Dry {
    sent: Arc<Mutex<Vec<String>>>,
}

impl Http for Dry {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.to_string());
        std::future::ready(Ok(Response {
            status: 200,
            headers: Vec::new(),
            body: br#"{"id":"0"}"#.to_vec(),
        }))
    }
}

/// A dry run's records, kept in memory, and every route connected to a
/// [`Dry`] that keeps what it is sent in `sent`.
async fn dry<'a>(
    config: &Config,
    vars: &BTreeMap<String, String>,
    sent: &Arc<Mutex<Vec<String>>>,
) -> Result<(Records<'a>, Connected<Connection>), String> {
    let secrets = EnvSecrets::from_vars(vars.clone());
    let prepared = prepare(config, &secrets, &RingSigner)
        .await
        .map_err(|error| error.to_string())?;
    let connected = prepared.connect(|_| {
        Ok::<_, String>(Connection::new(Dry {
            sent: Arc::clone(sent),
        }))
    })?;
    Ok((
        Records::Memory(herald_records::Kept::new(Memory::default())),
        connected,
    ))
}

/// A new marker secret printed once, or why none could be made.
fn init(secret: Option<String>, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let Some(secret) = secret else {
        say(
            err,
            "the system's random generator failed, so no secret was made",
        );
        return STOPPED;
    };
    say(out, &format!("HERALD_MARKER_SECRET={secret}"));
    say(
        out,
        "Store it as the secret HERALD_MARKER_SECRET, and name it in herald.toml as [markers] key = { secret = \"HERALD_MARKER_SECRET\" }. herald prints it only this once.",
    );
    DONE
}

/// Each route's templates rendered against the release of a forge's event
/// file, or a sample release, as its destination would show them.
fn previewing(
    config: &Config,
    route: Option<&str>,
    event_file: Option<(Forge, &str)>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let events = match event_file {
        Some((forge, path)) => {
            let read = std::fs::read(path)
                .map_err(|error| format!("{path} could not be read: {error}"))
                .and_then(|payload| forge.event(&payload).map_err(|error| error.to_string()));
            match read {
                Ok(Some(event)) => vec![event],
                Ok(None) => {
                    say(err, &format!("{path} publishes no release"));
                    return MISTAKEN;
                }
                Err(message) => {
                    say(err, &message);
                    return MISTAKEN;
                }
            }
        }
        None => samples(Kind::Release).into_iter().take(1).collect(),
    };
    let only = route.map(RouteId::new);
    let mut code = DONE;
    for event in &events {
        let previewed = preview(config, event, only.as_ref());
        if previewed.is_empty() {
            say(
                err,
                &format!("no route has the id \"{}\"", route.unwrap_or_default()),
            );
            return MISTAKEN;
        }
        for shown in previewed {
            let head = format!("{} to {}", shown.route.as_str(), shown.destination.as_str());
            match shown.rendered {
                Ok(announcement) => {
                    say(out, &format!("{head}:"));
                    say(out, &format!("  title: {}", announcement.title));
                    if let Some(thread) = &announcement.thread {
                        say(out, &format!("  thread: {thread}"));
                    }
                    for (index, part) in announcement.parts.iter().enumerate() {
                        say(out, &format!("  part {}:", index + 1));
                        for line in part.body.lines() {
                            say(out, &format!("    {line}"));
                        }
                    }
                }
                Err(reason) => {
                    code = STOPPED;
                    say(
                        out,
                        &format!("{head}: the templates cannot render this event: {reason}"),
                    );
                }
            }
        }
    }
    code
}

/// The release the run's own event publishes, if it does.
fn evented(
    forge: Forge,
    vars: &BTreeMap<String, String>,
    out: &mut dyn Write,
) -> Result<Option<Event>, String> {
    let event_name = variable(vars, "GITHUB_EVENT_NAME")?;
    if event_name != "release" {
        say(
            out,
            &format!("The run's event is {event_name}, not a release; nothing to announce."),
        );
        return Ok(None);
    }
    let path = variable(vars, "GITHUB_EVENT_PATH")?;
    let payload =
        std::fs::read(path).map_err(|error| format!("{path} could not be read: {error}"))?;
    let event = forge.event(&payload).map_err(|error| error.to_string())?;
    if event.is_none() {
        say(
            out,
            "The release is not published, or is a draft; nothing to announce.",
        );
    }
    Ok(event)
}

/// The GitLab CI pipeline's project, as its variables name it.
fn project(vars: &BTreeMap<String, String>) -> Result<Repository, String> {
    let id = variable(vars, "CI_PROJECT_ID")?;
    Ok(Repository {
        id: id
            .parse()
            .map_err(|_| format!("CI_PROJECT_ID is {id}, not a project's id"))?,
        name: variable(vars, "CI_PROJECT_NAME")?.to_owned(),
    })
}

/// The release tagged `tag` of the repository `options` names on `forge`,
/// asked for with the source's token, or else the run's own; the
/// repository asked for too where the run does not name it.
async fn tagged(
    options: &SourceOptions,
    forge: Forge,
    tag: &str,
    known: Option<Repository>,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<Option<Event>, String> {
    let secrets = EnvSecrets::from_vars(vars.clone());
    let token = match &options.token {
        Some(named) => {
            Some(Token::Named(secrets.secret(&named.secret).await.map_err(
                |_| format!("{} is not set; the source names it", named.secret),
            )?))
        }
        None => secrets
            .secret(Ci::of(forge).token)
            .await
            .ok()
            .map(Token::Run),
    };
    let origin = Origin::new(forge, options);
    let http = connect(origin.api())?;
    let asked = |request| async { http.send(request).await.map_err(|error| error.to_string()) };
    let answer = asked(origin.release_request(tag, token.as_ref())).await?;
    let repository = if let Some(repository) = known {
        repository
    } else {
        let described = asked(origin.repository_request(token.as_ref())).await?;
        forge
            .repository(&described)
            .map_err(|error| error.to_string())?
    };
    let event = forge
        .release(&repository, &answer)
        .map_err(|error| error.to_string())?;
    if event.is_none() {
        say(
            out,
            &format!("The release {tag} is a draft; nothing to announce."),
        );
    }
    Ok(event)
}

/// A command's work, over whichever store the config names.
trait Served {
    /// What the work did.
    type Done;

    /// The work done over `store`.
    fn over<S: Keeps>(&self, store: &S) -> impl Future<Output = Self::Done>;
}

/// An event, and the routes and sources it is announced through.
struct Announcing<'a> {
    config: &'a Config,
    connected: &'a Connected<Connection>,
    sources: &'a [&'a SourceName],
    event: &'a Event,
    run: &'a str,
}

impl Served for Announcing<'_> {
    type Done = Vec<Announced>;

    async fn over<S: Keeps>(&self, store: &S) -> Vec<Announced> {
        let ports = Ports {
            store,
            clock: &SystemClock,
            signer: &RingSigner,
        };
        let mut all = Vec::new();
        for source in self.sources {
            all.extend(
                announce(
                    self.config,
                    self.connected,
                    &ports,
                    source,
                    self.event,
                    self.run,
                )
                .await,
            );
        }
        all
    }
}

/// Every source polled through the routes and sources connected.
struct Polling<'a> {
    config: &'a Config,
    connected: &'a Connected<Connection>,
    run: &'a str,
}

impl Served for Polling<'_> {
    type Done = Vec<Swept>;

    async fn over<S: Keeps>(&self, store: &S) -> Vec<Swept> {
        let ports = Ports {
            store,
            clock: &SystemClock,
            signer: &RingSigner,
        };
        poll(self.config, self.connected, &ports, self.run).await
    }
}

/// Every chore that fell due, done through the routes connected.
struct Housekeeping<'a> {
    connected: &'a Connected<Connection>,
}

impl Served for Housekeeping<'_> {
    type Done = Result<Vec<Kept>, Stop>;

    async fn over<S: Keeps>(&self, store: &S) -> Self::Done {
        housekeep(self.connected, store, &SystemClock).await
    }
}

/// The name this run takes its leases under.
fn run_name(vars: &BTreeMap<String, String>) -> String {
    let pair = |first: &str, second: &str| vars.get(first).zip(vars.get(second));
    if let Some((id, attempt)) = pair("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT") {
        format!("github/{id}/{attempt}")
    } else if let Some((pipeline, job)) = pair("CI_PIPELINE_ID", "CI_JOB_ID") {
        format!("gitlab/{pipeline}/{job}")
    } else {
        "local".to_owned()
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
    let polling = Polling {
        config,
        connected: &connected,
        run: &run,
    };
    let swept = records.serve(&connected, &polling).await;
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
enum Records<'a> {
    /// On a git branch.
    Branch(GitBranchStore<Connection>),
    /// In the Actions cache.
    Cache(ActionsCacheStore<'a, Connection>),
    /// In the repository's variables.
    Variables(RepoVariablesStore<Connection>),
    /// In the routes' own posts.
    Posts,
    /// In memory, for a dry run.
    Memory(herald_records::Kept<Memory>),
}

impl Records<'_> {
    /// `work` done over the store, the routes' posts made one where they
    /// keep the records.
    async fn serve<W: Served>(&self, connected: &Connected<Connection>, work: &W) -> W::Done {
        match self {
            Self::Branch(store) => work.over(store).await,
            Self::Cache(store) => work.over(store).await,
            Self::Variables(store) => work.over(store).await,
            Self::Posts => work.over(&connected.marked(RingSigner)).await,
            Self::Memory(store) => work.over(store).await,
        }
    }
}

/// The store kinds this herald keeps records in.
const STORES: [&str; 4] = ["git-branch", "markers", "actions-cache", "repo-variables"];

/// The run's records, and every route and source connected.
async fn reached<'a>(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'a>,
) -> Result<(Records<'a>, Connected<Connection>), String> {
    let secrets = EnvSecrets::from_vars(vars.clone());
    let records = match &config.store {
        Store::GitBranch(options) => {
            Records::Branch(branch(options, vars, &secrets, connect).await?)
        }
        Store::ActionsCache(options) => {
            Records::Cache(cache(options, vars, &secrets, connect).await?)
        }
        Store::RepoVariables(options) => {
            Records::Variables(variables(options, vars, &secrets, connect).await?)
        }
        Store::Markers => Records::Posts,
        _ => return Err(unserved(&STORES)),
    };
    let prepared = prepare(config, &secrets, &RingSigner)
        .await
        .map_err(|error| error.to_string())?;
    Ok((records, prepared.connect(connect)?))
}

/// The repository a store names, or else the run's own.
fn repository<'a>(
    named: Option<&'a str>,
    vars: &'a BTreeMap<String, String>,
) -> Result<&'a str, String> {
    named.map_or_else(|| variable(vars, "GITHUB_REPOSITORY"), Ok)
}

async fn branch(
    options: &BranchOptions,
    vars: &BTreeMap<String, String>,
    secrets: &EnvSecrets,
    connect: Connect<'_>,
) -> Result<GitBranchStore<Connection>, String> {
    let repository = repository(options.repository.as_deref(), vars)?;
    let token_name = options
        .token
        .as_ref()
        .map_or(GITHUB_TOKEN, |token| token.secret.as_str());
    let token = secrets.secret(token_name).await.map_err(|_| {
        format!("{token_name} is not set; the git-branch store needs a token that may push")
    })?;
    let branch = GitBranch::new(
        repository,
        token,
        options.branch.as_deref(),
        options.api.as_deref(),
    );
    let http = connect(branch.api())?;
    Ok(branch.store(http))
}

async fn cache<'a>(
    options: &CacheOptions,
    vars: &BTreeMap<String, String>,
    secrets: &EnvSecrets,
    connect: Connect<'a>,
) -> Result<ActionsCacheStore<'a, Connection>, String> {
    let results = variable(vars, RESULTS)?;
    let token = secrets.secret(RUNTIME_TOKEN).await.map_err(|_| {
        format!("{RUNTIME_TOKEN} is not set; the actions-cache store runs through herald's Action")
    })?;
    let cache = ActionsCache::new(results, token, options.prefix.as_deref());
    let http = connect(cache.results())?;
    Ok(cache.store(http, Box::new(connect)))
}

async fn variables(
    options: &VariablesOptions,
    vars: &BTreeMap<String, String>,
    secrets: &EnvSecrets,
    connect: Connect<'_>,
) -> Result<RepoVariablesStore<Connection>, String> {
    let repository = repository(options.repository.as_deref(), vars)?;
    let name = options.token.secret.as_str();
    let token = secrets.secret(name).await.map_err(|_| {
        format!("{name} is not set; the repo-variables store needs a token with the repository's Variables permission")
    })?;
    let variables = RepoVariables::new(repository, token, options.api.as_deref());
    let http = connect(variables.api())?;
    Ok(variables.store(http))
}

async fn housekeeping(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (records, connected) = reached(config, vars, connect).await?;
    let housekeeping = Housekeeping {
        connected: &connected,
    };
    let kept = records
        .serve(&connected, &housekeeping)
        .await
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
