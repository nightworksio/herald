//! herald's CLI (ADR-0009): `herald announce --from github`, `--from gitlab`
//! or `--from forgejo` in a forge's CI, `herald poll` and `herald housekeep`
//! on a schedule, `herald server` as a long-running endpoint, and `herald
//! doctor` anywhere.
//!
//! [`run_until`] is the whole CLI, handed its arguments, its environment, the
//! way to reach each base URL and what stops a server, so it runs the same
//! under test as on a runner.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::future::Future;
use std::io::Write;
use std::ops::ControlFlow;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

use clap::Parser as _;
use herald_config::{Config, Uses, load};
use herald_flows::{Chored, Remembered, Sweep};
use herald_native::random_secret;
use herald_sources_forge::Forge;

mod announce;
mod apps;
mod cli;
mod connect;
mod doctoring;
mod dry_run;
mod followers;
mod health;
mod lambda;
mod output;
mod preview;
mod release;
mod serving;
mod setup;
mod signals;
mod stores;

pub use connect::{Connect, Connection, Connector, NATIVE, native, waiting};
pub use doctoring::schema as doctor_schema;
pub use signals::stopped;

use announce::{Announce, Routed, announcing, announcing_from};
use cli::{Cli, Command, Reading, RunForge, Running, Tokens};
use doctoring::DoctorArgs;
use herald_compose::told::{chored, polled, skipped, why};
use herald_compose::{GITHUB_TOKEN, Hosting};
use output::{Scrubbing, hidden, masked, say, warned};
use preview::previewing;
use serving::serving;
use stores::composed;

/// Every route announced, already announced, or held by another run; every
/// source polled and its cursor moved on; or every chore that fell due done
/// or dropped.
pub const DONE: u8 = 0;
/// A route stopped part-way, a source waits for the next poll, a chore
/// waits for the next run, or the run could not start.
pub(crate) const STOPPED: u8 = 1;
/// The config has mistakes.
pub(crate) const MISTAKEN: u8 = 2;

/// A GitLab CI job's own token.
const JOB_TOKEN: &str = "CI_JOB_TOKEN";
/// Where `herald server` listens, in place of the config's.
const LISTEN: &str = "HERALD_LISTEN";
/// What bounds a request's whole time, in milliseconds, in place of the
/// config's `[http] total`.
const TIMEOUT_MS: &str = "HERALD_TIMEOUT_MS";
/// The fewest characters of a value scrubbed from output: a shorter one
/// would rewrite ordinary words, and is no credential.
const SCRUBBED: usize = 8;

/// `made`, its error worded as herald says it.
fn worded<T>(made: Result<T, impl Display>) -> Result<T, String> {
    made.map_err(|error| error.to_string())
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
    Box::pin(stopping(args, vars, connect, out, err, Box::pin(stop))).await
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
    let cli = match parsed(args, out, err) {
        Ok(cli) => cli,
        Err(code) => return code,
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
    let doctored = match reading {
        Reading::Doctor(asked) => Some(asked),
        _ => None,
    };
    let config = match configured(&cli.config, doctored, out, err) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let within = connect.within(
        config
            .http
            .clone()
            .timed(vars.get(TIMEOUT_MS).map(String::as_str)),
    );
    let connect = &within;
    let hidden = hidden(&config, vars);
    let json = doctored.is_some_and(|asked| asked.format == doctoring::Format::Json);
    masked(&hidden, vars, if json { &mut *err } else { &mut *out });
    let out = &mut Scrubbing::new(out, &hidden);
    let err = &mut Scrubbing::new(err, &hidden);
    let run = match answered(reading, &config, &cli.config, vars, connect, out, err).await {
        ControlFlow::Continue(run) => run,
        ControlFlow::Break(code) => return code,
    };
    if let Some(layer) = doctoring::warning(&run, vars) {
        warned(&doctoring::gaps(&config, layer, vars).await, vars, out);
    }
    let code = finished(ran(run, config, vars, connect, out, stop).await, err);
    let _ = out.flush();
    let _ = err.flush();
    code
}

/// The CLI `args` ask for, or the code it ends with once its help, version
/// or mistake is printed.
fn parsed(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Result<Cli, u8> {
    Cli::try_parse_from(args).map_err(|error| {
        let rendered = error.render();
        let _ = if error.use_stderr() {
            write!(err, "{rendered}")
        } else {
            write!(out, "{rendered}")
        };
        u8::try_from(error.exit_code()).unwrap_or(MISTAKEN)
    })
}

/// The config at `path`, or the code a run ends with where it cannot be
/// read: the doctor's findings where `doctored` asks for them, or else why.
fn configured(
    path: &str,
    doctored: Option<DoctorArgs>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Config, u8> {
    read(path).map_err(|message| {
        if let Some(code) = doctored.and_then(|asked| doctoring::unread(asked, &message, out)) {
            return code;
        }
        say(err, &message);
        MISTAKEN
    })
}

/// The command that runs against the config, run until it is done or
/// `stop` completes.
async fn ran(
    run: Running,
    config: Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
    stop: Pin<Box<dyn Future<Output = ()> + '_>>,
) -> Result<u8, String> {
    match run {
        Running::Announce {
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
                    forge: from.map_or(Forge::GitHub, RunForge::forge),
                    release: release.as_deref(),
                    dry_run,
                };
                announcing_from(&config, vars, connect, announcing, out).await
            }
        }
        Running::Poll => polling(&config, vars, connect, out).await,
        Running::Housekeep => housekeeping(&config, vars, connect, out).await,
        Running::Server { listen } => {
            serving(&config, vars, connect, listen.as_deref(), out, stop).await
        }
        Running::Lambda => lambda::lambda(config, vars, connect, out).await,
        Running::Followers(asked) => {
            followers::following(&config, vars, connect, &asked, out).await
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
/// with its exit code, or continues with the command that runs against it.
async fn answered(
    reading: Reading,
    config: &Config,
    path: &str,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ControlFlow<u8, Running> {
    ControlFlow::Break(match reading {
        Reading::Doctor(asked) => {
            doctoring::doctoring(config, path, asked, vars, connect, out).await
        }
        Reading::Store => {
            say(out, config.store.kind());
            DONE
        }
        Reading::Preview { route, event, from } => {
            let event = event.as_deref().map(|path| (from.forge(), path));
            previewing(config, route.as_deref(), event, out, err)
        }
        Reading::Set(setting) => {
            let done = setup::set_up(config, vars, connect, &setting, &random_secret, out).await;
            finished(done, err)
        }
        Reading::Run(run) => return ControlFlow::Continue(run),
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
    Box::pin(run_until(
        args,
        vars,
        connect,
        out,
        err,
        std::future::pending(),
    ))
    .await
}

fn read(path: &str) -> Result<Config, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("{path} could not be read: {error}"))?;
    let directory = Path::new(path).parent().unwrap_or(Path::new(""));
    let files = |file: &str| {
        std::fs::read_to_string(directory.join(file)).map_err(|error| error.to_string())
    };
    let mut config =
        load(path, &text, &files, &herald_compose::kinds()).map_err(|errors| errors.to_string())?;
    if let Some(options) = config.store.of::<herald_store_sqlite::StoreOptions>() {
        let mut options = options.clone();
        options.path = Some(options.file(directory).display().to_string());
        config.store = herald_config::ConfiguredStore::new(options);
    }
    Ok(config)
}

/// The secret `herald init` names for the marker key it prints.
const MARKER_SECRET: &str = "HERALD_MARKER_SECRET";

/// A new marker secret printed once, or why none could be made.
fn init(secret: Option<String>, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let Some(secret) = secret else {
        say(
            err,
            "the system's random generator failed, so no secret was made",
        );
        return STOPPED;
    };
    say(out, &format!("{MARKER_SECRET}={secret}"));
    say(
        out,
        &format!(
            "Store it as the secret {MARKER_SECRET}, and name it in herald.toml as [markers] key = {{ secret = \"{MARKER_SECRET}\" }}. herald prints it only this once."
        ),
    );
    DONE
}

/// Where this run takes its leases: the CI run and job it is in, or
/// `local`; each job then names its leases with a nonce no other takes, so
/// two jobs of one CI run never share them.
fn place(vars: &BTreeMap<String, String>) -> String {
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
    let composed = composed(
        config,
        vars,
        connect,
        Hosting::Actions,
        Uses::Polls,
        place(vars),
    )
    .await?;
    let swept = composed.poll().await;
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
    let composed = composed(
        config,
        vars,
        connect,
        Hosting::Actions,
        Uses::Announces,
        place(vars),
    )
    .await?;
    let kept = composed.housekeep().await.map_err(|stop| why(&stop))?;
    if kept.kept.is_empty() {
        say(out, "No chore is due.");
    }
    let mut code = DONE;
    for chore in &kept.kept {
        if matches!(chore.outcome, Chored::Later(_)) {
            code = STOPPED;
        }
        say(out, &chored(chore));
    }
    for path in &kept.skipped {
        say(out, &skipped(path));
    }
    Ok(code)
}

#[cfg(test)]
mod tests;
