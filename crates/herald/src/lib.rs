//! herald's CLI (ADR-0009): `herald announce --from github`, `--from gitlab`
//! or `--from forgejo` in a forge's CI, `herald poll` and `herald housekeep`
//! on a schedule, `herald server` as a long-running endpoint, and `herald
//! doctor` anywhere.
//!
//! [`run_until`] is the whole CLI, handed its arguments, its environment, the
//! way to reach each base URL and what stops a server, so it runs the same
//! under test as on a runner.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fmt::Display;
use std::future::Future;
use std::io::Write;
use std::ops::ControlFlow;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clap::Parser as _;
use herald_config::{Config, Source, load, samples};
use herald_core::{Event, Kind, RouteId, SourceName};
use herald_crypto::RingSigner;
use herald_environment::{EnvSecrets, SystemClock};
use herald_flows::{
    Announced, Chored, Connected, Findings, Gap, Kept, Outcome, Ports, Remembered, Stop, Sweep,
    Swept, announce_within, doctor, housekeep, keys, poll, prepare, preview,
};
use herald_native::{ConnectError, NativeHttp, PublicHttp, random_secret};
use herald_ports::{Http, HttpError, Keyring, PUBLIC, Request, Response, Secrets, Store as Keeps};
use herald_records::Memory;
use herald_sources_forge::{Forge, Origin, Repository, SourceOptions, Token};

mod apps;
mod cli;
mod health;
mod lambda;
mod output;
mod serving;
mod setup;
mod stores;

use cli::{
    Cli, Command, Discording, From, Layer, Matrixing, Reading, Serve, Setting, Slacking, Tokens,
};
use herald_compose::told::{chored, polled, report, why};
use herald_compose::{GITHUB_TOKEN, Served};
use output::{Scrubbing, hidden, masked, say, warned};
use serving::serving;
use stores::{Commands, Records, STORES, reached};

/// Every route announced, already announced, or held by another run; every
/// source polled and its cursor moved on; or every chore that fell due done
/// or dropped.
pub const DONE: u8 = 0;
/// A route stopped part-way, a source waits for the next poll, a chore
/// waits for the next run, or the run could not start.
pub const STOPPED: u8 = 1;
/// The config has mistakes.
pub const MISTAKEN: u8 = 2;

/// Where the Actions cache's results service is, as herald's Action hands
/// it on.
const RESULTS: &str = "ACTIONS_RESULTS_URL";
/// The run's token for the Actions cache, as herald's Action hands it on.
const RUNTIME_TOKEN: &str = "ACTIONS_RUNTIME_TOKEN";
/// A GitLab CI job's own token.
const JOB_TOKEN: &str = "CI_JOB_TOKEN";
/// Where `herald server` listens, in place of the config's.
const LISTEN: &str = "HERALD_LISTEN";
/// The fewest characters of a value scrubbed from output: a shorter one
/// would rewrite ordinary words, and is no credential.
const SCRUBBED: usize = 8;

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

/// The native connection to `base`: one reaching only its origin, or, for
/// [`PUBLIC`], one reaching any public address (ADR-0007).
///
/// # Errors
///
/// When `base` may not be reached, or the client cannot be built.
pub fn native(base: &str) -> Result<Connection, String> {
    let connection = if base == PUBLIC {
        PublicHttp::new()
            .map(Connection::new)
            .map_err(ConnectError::Build)
    } else if let Some(waiting) = base.strip_prefix(lambda::WAITING) {
        NativeHttp::waiting(waiting).map(Connection::new)
    } else {
        NativeHttp::reaching(base).map(Connection::new)
    };
    worded(connection)
}

/// `made`, its error worded as herald says it.
fn worded<T>(made: Result<T, impl Display>) -> Result<T, String> {
    made.map_err(|error| error.to_string())
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
                token: JOB_TOKEN,
            },
        }
    }
}

/// The CLI, run with `args`, in an environment of `vars`, reaching each base
/// URL through what `connect` makes for it, and writing to `out` and `err`.
/// It answers the process's exit code. A server it runs runs until `stop`
/// completes.
pub async fn run_until(
    args: &[String],
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    stop: impl Future<Output = ()>,
) -> u8 {
    stopping(args, vars, connect, out, err, Box::pin(stop)).await
}

/// [`run_until`] with `stop` boxed, so one CLI serves whatever stops it.
async fn stopping(
    args: &[String],
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    stop: Pin<Box<dyn Future<Output = ()> + '_>>,
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
        Command::Token(Tokens::New { app }) => return apps::token(&app, random_secret(), out, err),
        Command::Health { url, timeout } => {
            let listen = vars.get(LISTEN).map(String::as_str);
            let configured = || read(&cli.config).ok().map(|config| config.server.listen);
            let url = health::url(url, listen, configured);
            return health::health(&url, Duration::from_millis(timeout), out, err).await;
        }
        Command::Read(reading) => reading,
    };
    let config = match read(&cli.config) {
        Ok(config) => config,
        Err(message) => {
            say(err, &message);
            return MISTAKEN;
        }
    };
    let hidden = hidden(&config, vars);
    masked(&hidden, vars, out);
    let out = &mut Scrubbing::new(out, &hidden);
    let err = &mut Scrubbing::new(err, &hidden);
    let serve = match answered(reading, &config, &cli.config, vars, connect, out, err).await {
        ControlFlow::Continue(serve) => serve,
        ControlFlow::Break(code) => return code,
    };
    let layer = match serve {
        Serve::Server { .. } => Layer::Server,
        Serve::Lambda => Layer::Lambda,
        _ => Layer::Actions,
    };
    warned(&doctor(&config, layer.served()).gaps, vars, out);
    let ran = match serve {
        Serve::Announce {
            from,
            release,
            event,
            app,
            dry_run,
        } => {
            if let (Some(event), Some(app)) = (&event, &app) {
                let written = apps::Written {
                    app,
                    event,
                    dry_run,
                };
                apps::announcing_event(&config, vars, connect, written, out).await
            } else {
                let announcing = Announce {
                    forge: from.map_or(Forge::GitHub, From::forge),
                    release: release.as_deref(),
                    dry_run,
                };
                announcing_from(&config, vars, connect, announcing, out).await
            }
        }
        Serve::Poll => polling(&config, vars, connect, out).await,
        Serve::Housekeep => housekeeping(&config, vars, connect, out).await,
        Serve::Server { listen } => {
            serving(&config, vars, connect, listen.as_deref(), out, stop).await
        }
        Serve::Lambda => lambda::lambda(config, vars, connect, out).await,
    };
    let code = ran.unwrap_or_else(|message| {
        say(err, &message);
        STOPPED
    });
    let _ = out.flush();
    let _ = err.flush();
    code
}

/// `config`'s destination set up as `setting` asks.
async fn set_up(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    setting: Setting,
    out: &mut dyn Write,
) -> Result<u8, String> {
    match setting {
        Setting::Discord(Discording::Commands { destination }) => {
            let asked = setup::Discording::Commands;
            setup::discord(config, vars, connect, destination.as_deref(), asked, out).await
        }
        Setting::Discord(Discording::Roles {
            destination,
            guild,
            create,
        }) => {
            let asked = setup::Discording::Roles {
                guild: &guild,
                create: create.as_deref(),
            };
            setup::discord(config, vars, connect, destination.as_deref(), asked, out).await
        }
        Setting::Slack(Slacking::Manifest { destination, url }) => {
            setup::slack(config, destination.as_deref(), url.as_deref(), out)
        }
        Setting::Matrix(Matrixing::Registration { destination, url }) => {
            setup::matrix(config, destination.as_deref(), &url, &random_secret, out)
        }
    }
}

/// The code a setup command ends with, saying why where it stopped.
fn finished(done: Result<u8, String>, err: &mut dyn Write) -> u8 {
    done.unwrap_or_else(|message| {
        say(err, &message);
        STOPPED
    })
}

/// Answers a command that reads the config at `path` and finishes, breaking
/// with its exit code, or continues with the command that serves.
async fn answered(
    reading: Reading,
    config: &Config,
    path: &str,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ControlFlow<u8, Serve> {
    ControlFlow::Break(match reading {
        Reading::Doctor { layer } => {
            let mut findings = doctor(config, layer.served());
            let keyed = keys(config, &EnvSecrets::from_vars(vars.clone()), &RingSigner).await;
            findings.notes.extend(keyed.notes);
            findings.mistakes.extend(keyed.mistakes);
            checked(path, &findings, out)
        }
        Reading::Store => {
            say(out, config.store.kind());
            DONE
        }
        Reading::Preview { route, event, from } => {
            let event = event.as_deref().map(|path| (from.forge(), path));
            previewing(config, route.as_deref(), event, out, err)
        }
        Reading::Set(setting) => finished(set_up(config, vars, connect, setting, out).await, err),
        Reading::Serve(serve) => return ControlFlow::Continue(serve),
    })
}

/// [`run_until`], with a server it runs running until the process ends.
pub async fn run(
    args: &[String],
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    run_until(args, vars, connect, out, err, std::future::pending()).await
}

fn read(path: &str) -> Result<Config, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("{path} could not be read: {error}"))?;
    let directory = Path::new(path).parent().unwrap_or(Path::new(""));
    let files = |file: &str| {
        std::fs::read_to_string(directory.join(file)).map_err(|error| error.to_string())
    };
    let mut config = load(path, &text, &files).map_err(|errors| errors.to_string())?;
    if let herald_config::Store::Sqlite(options) = &mut config.store {
        options.path = Some(options.file(directory).display().to_string());
    }
    Ok(config)
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
    for mistake in &findings.mistakes {
        say(out, mistake);
    }
    if !findings.mistakes.is_empty() {
        MISTAKEN
    } else if findings.gaps.is_empty() {
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
    let routed = Routed {
        sources: &sources,
        within: None,
        event: &event,
    };
    announcing(config, vars, connect, routed, asked.dry_run, out).await
}

/// An event, and the sources and routes it is announced from and to.
struct Routed<'a> {
    /// The sources it comes from.
    sources: &'a [&'a SourceName],
    /// The routes it may go to, where only some may take it.
    within: Option<&'a [RouteId]>,
    /// The event.
    event: &'a Event,
}

/// `routed` announced to every route that takes it, or, on a dry run, the
/// requests a first announcement sends printed and nothing sent.
async fn announcing(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    routed: Routed<'_>,
    dry_run: bool,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let (records, connected) = if dry_run {
        dry(config, vars, &sent).await?
    } else {
        reached(config, vars, connect, Commands::Unanswered, &STORES).await?
    };
    let run = run_name(vars);
    let announcing = Announcing {
        config,
        connected: &connected,
        routed,
        run: &run,
    };
    let announced = records.serve(&connected, announcing).await;
    if dry_run {
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
            .push(request.with_body().to_string());
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
    let Ok(connected) = prepared.connect(|_| {
        Ok::<_, Infallible>(Connection::new(Dry {
            sent: Arc::clone(sent),
        }))
    });
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
                    say(out, &format!("  title: {}", announcement.title.plain()));
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

/// An event, and the routes and sources it is announced through.
struct Announcing<'a> {
    config: &'a Config,
    connected: &'a Connected<Connection>,
    routed: Routed<'a>,
    run: &'a str,
}

impl Served for Announcing<'_> {
    type Done = Vec<Announced>;

    async fn over<S: Keeps + Keyring>(self, store: &S) -> Vec<Announced> {
        let ports = Ports {
            store,
            clock: &SystemClock,
            signer: &RingSigner,
        };
        let mut all = Vec::new();
        for source in self.routed.sources {
            all.extend(
                announce_within(
                    self.config,
                    self.connected,
                    &ports,
                    source,
                    self.routed.event,
                    self.routed.within,
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

    async fn over<S: Keeps + Keyring>(self, store: &S) -> Vec<Swept> {
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

    async fn over<S: Keeps + Keyring>(self, store: &S) -> Self::Done {
        housekeep(self.connected, store, &SystemClock).await
    }
}

/// The name this run takes its leases under: where it runs, then a nonce no
/// other run takes, so two jobs of one CI run never share it.
fn run_name(vars: &BTreeMap<String, String>) -> String {
    let pair = |first: &str, second: &str| vars.get(first).zip(vars.get(second));
    let place = if let Some((id, attempt)) = pair("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT") {
        format!("github/{id}/{attempt}")
    } else if let Some((pipeline, job)) = pair("CI_PIPELINE_ID", "CI_JOB_ID") {
        format!("gitlab/{pipeline}/{job}")
    } else {
        "local".to_owned()
    };
    format!("{place}/{}", herald_environment::nonce())
}

async fn polling(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (records, connected) =
        reached(config, vars, connect, Commands::Unanswered, &STORES).await?;
    let run = run_name(vars);
    let polling = Polling {
        config,
        connected: &connected,
        run: &run,
    };
    let swept = records.serve(&connected, polling).await;
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

async fn housekeeping(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let (records, connected) =
        reached(config, vars, connect, Commands::Unanswered, &STORES).await?;
    let housekeeping = Housekeeping {
        connected: &connected,
    };
    let kept = records
        .serve(&connected, housekeeping)
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

/// When the process is asked to stop, listened for from this call on: Ctrl-C,
/// or `SIGTERM` as a container runtime sends it where there are signals.
pub fn stopped() -> impl Future<Output = ()> {
    #[cfg(unix)]
    let asked = {
        use tokio::signal::unix::{SignalKind, signal};
        signalled(
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        )
    };
    #[cfg(not(unix))]
    let asked = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    asked
}

/// When either signal arrives, or at once where either cannot be listened
/// for.
#[cfg(unix)]
async fn signalled(
    terminate: std::io::Result<tokio::signal::unix::Signal>,
    interrupt: std::io::Result<tokio::signal::unix::Signal>,
) {
    if let (Ok(mut terminate), Ok(mut interrupt)) = (terminate, interrupt) {
        futures_util::future::select(
            std::pin::pin!(terminate.recv()),
            std::pin::pin!(interrupt.recv()),
        )
        .await;
    }
}

#[cfg(test)]
mod tests;
