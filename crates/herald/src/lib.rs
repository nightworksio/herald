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
use herald_config::{Config, Unread, Uses, Within, load_with};
use herald_core::Printed;
use herald_flows::{Chored, Remembered, Sweep};
use herald_native::random_secret;
use herald_sources_forge::Forge;

mod announce;
mod apps;
mod asking;
mod cli;
mod connect;
mod doctoring;
mod dry_run;
mod edited;
mod followers;
mod health;
mod init;
mod keygen;
mod lambda;
mod merging;
mod orphaning;
mod output;
mod pages;
mod peers;
mod presetting;
mod preview;
mod queue_setup;
mod queueing;
mod release;
mod rerunning;
mod serving;
mod setup;
mod signals;
mod sourcing;
mod stores;
mod withdrawing;

pub use connect::{Connect, Connection, Connector, NATIVE, native, waiting};
pub use doctoring::schema as doctor_schema;
pub use signals::stopped;

use announce::{Announce, Routed, announcing, announcing_from};
use cli::{Activitypub, Cli, Command, Reading, RunForge, Running, Tokens};
use doctoring::DoctorArgs;
use herald_compose::Log;
use herald_compose::told::{chored, polled_as, skipped, why};
use herald_compose::{GITHUB_TOKEN, Hosting};
use output::{Logged, Scrubbing, hidden, masked, say, warned};
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

/// `made`, its error worded as herald says it.
fn worded<T>(made: Result<T, impl Display>) -> Result<T, String> {
    made.map_err(|error| error.to_string())
}

/// The CLI, run with `args`, in an environment of `vars`, reaching each base
/// URL through what `connect` makes for it, and writing to `out` and `err`.
/// It answers the process's exit code. A server it runs runs until `stop`
/// completes, and fails where it completes with why.
pub async fn run_until(
    args: &[String],
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    stop: impl Future<Output = Result<(), String>>,
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
    stop: Pin<Box<dyn Future<Output = Result<(), String>> + '_>>,
) -> u8 {
    let cli = match parsed(args, out, err) {
        Ok(cli) => cli,
        Err(code) => return code,
    };
    let reading = match cli.command {
        Command::Init(asked) => {
            let answers = &mut tokio::io::BufReader::new(tokio::io::stdin());
            let done = init::initialising(&cli.config, &asked, (answers, out), random_secret);
            return finished(done.await, err);
        }
        Command::Token(Tokens::Marker) => return marker(random_secret(), out, err),
        Command::Token(Tokens::New { app }) => return apps::token(&app, random_secret(), out, err),
        Command::Health { url, timeout } => {
            let listen = vars.get(LISTEN).map(String::as_str);
            let configured = || read(&cli.config).ok().map(|config| config.server.listen);
            let url = health::url(url, listen, configured);
            return health::health(&url, Duration::from_millis(timeout), out, err).await;
        }
        Command::Activitypub(Activitypub::Keygen { file, bits, force }) => {
            return keygen::keygen(&file, (bits.into(), force), random_secret(), out, err);
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
    let hidden = hidden(&config, vars);
    let scrubber = config.log.scrubber().timed(herald_environment::SystemClock);
    for secret in &hidden {
        scrubber.keep(secret);
    }
    let within = within.scrubbing(scrubber.clone());
    let connect = &within;
    let json = doctored.is_some_and(|asked| asked.format == doctoring::Format::Json);
    masked(&hidden, vars, if json { &mut *err } else { &mut *out });
    let out = &mut Scrubbing::new(out, scrubber.clone());
    let err = &mut Scrubbing::new(err, scrubber.clone());
    let log = match Log::of(
        &config.log,
        |name| var(vars, name),
        scrubber,
        herald_environment::SystemClock,
    ) {
        Ok(log) => log.flagged(cli.log_level, cli.log_format),
        Err(message) => {
            say(err, &message);
            return MISTAKEN;
        }
    };
    let run = match answered(reading, &config, &cli.config, vars, connect, out, err).await {
        ControlFlow::Continue(run) => run,
        ControlFlow::Break(code) => return code,
    };
    let done = {
        let out = &mut Logged::new(out, &log);
        out.told(&herald_compose::told::configured(&config));
        if let Some(layer) = doctoring::warning(&run, vars) {
            warned(&doctoring::gaps(&config, layer, vars).await, vars, out);
        }
        Box::pin(ran(run, config, vars, connect, out, stop)).await
    };
    let code = ended(done, &mut Logged::new(err, &log));
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
    out: &mut Logged<'_>,
    stop: Pin<Box<dyn Future<Output = Result<(), String>> + '_>>,
) -> Result<u8, String> {
    match run {
        Running::Announce {
            from,
            release,
            event,
            app,
            dry_run,
            private,
        } => {
            let printed = Printed::of(private, config.cli.private);
            if let (Some(event), Some(app)) = (&event, &app) {
                let written = apps::Written {
                    app,
                    event,
                    dry_run,
                    printed,
                };
                apps::announcing_event(&config, vars, connect, written, out).await
            } else {
                let announcing = Announce {
                    forge: from.map_or(Forge::GitHub, RunForge::forge),
                    release: release.as_deref(),
                    dry_run,
                    printed,
                };
                announcing_from(&config, vars, connect, announcing, out).await
            }
        }
        Running::Poll { private } => {
            let printed = Printed::of(private, config.cli.private);
            polling(&config, vars, connect, printed, out).await
        }
        Running::Housekeep => housekeeping(&config, vars, connect, out).await,
        Running::Server { listen } => {
            serving(&config, vars, connect, listen.as_deref(), out, stop).await
        }
        Running::Lambda => Box::pin(lambda::lambda(config, vars, connect, out)).await,
        Running::Followers(asked) => {
            followers::following(&config, vars, connect, &asked, out.out()).await
        }
        Running::Withdraw {
            names,
            every_route,
            dry_run,
        } => {
            let asked = withdrawing::Asked::of(&names, every_route, dry_run)?;
            withdrawing::withdrawing(&config, vars, connect, asked, out).await
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

/// The code a run that tells what it did ends with, logging why where it
/// stopped.
fn ended(done: Result<u8, String>, err: &mut Logged<'_>) -> u8 {
    done.unwrap_or_else(|message| {
        err.told(&herald_compose::Told::error(message));
        STOPPED
    })
}

/// The variable `name`, where `vars` set it and it is not empty.
fn var<'v>(vars: &'v BTreeMap<String, String>, name: &str) -> Option<&'v str> {
    vars.get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
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
        Reading::Orphans(cli::Orphaning::Clear {
            report,
            route,
            seen,
            ..
        }) => {
            let asked = (report.as_str(), route.as_deref(), seen.as_deref());
            let done = orphaning::cleared(config, vars, connect, asked, out);
            finished(done.await, err)
        }
        Reading::Rerun {
            rule,
            apply,
            job,
            private,
            ..
        } => {
            let printed = Printed::of(private, config.cli.private);
            let asked = ((rule.as_deref(), printed), apply, job.as_slice());
            let done = rerunning::rerunning(config, vars, connect, asked, out);
            finished(done.await, err)
        }
        Reading::MergeQueue {
            queue,
            apply,
            private,
        } => {
            let printed = Printed::of(private, config.cli.private);
            let asked = ((queue.as_deref(), printed), apply);
            let done = merging::merging(config, vars, connect, asked, out);
            finished(done.await, err)
        }
        Reading::Queue {
            view,
            format,
            private,
        } => {
            let asked = (
                view.as_deref(),
                format,
                Printed::of(private, config.cli.private),
            );
            let done = queueing::queueing(config, vars, connect, asked, out);
            finished(done.await, err)
        }
        Reading::Preview {
            route,
            event,
            from,
            group,
        } => {
            let event = event.as_deref().map(|path| (from.forge(), path));
            previewing(config, route.as_deref(), event, group.as_deref(), out, err)
        }
        Reading::Set(setting) => {
            let done = setup::set_up(config, vars, connect, &setting, &random_secret, out).await;
            finished(done, err)
        }
        Reading::Setup(cli::SettingUp::MergeQueue(asked)) => {
            let answers = &mut tokio::io::BufReader::new(tokio::io::stdin());
            let done =
                queue_setup::setting_up(config, path, (vars, connect), &asked, answers, out).await;
            finished(done, err)
        }
        Reading::Preset(asked) => {
            let answers = &mut tokio::io::BufReader::new(tokio::io::stdin());
            finished(
                presetting::presetting(path, &asked, answers, out).await,
                err,
            )
        }
        Reading::Peer(asked) => {
            let answers = &mut tokio::io::BufReader::new(tokio::io::stdin());
            let done = peers::peering(config, path, &asked, (vars, connect), answers, out).await;
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

/// `done`, its error worded after `what`, as herald says a file could not
/// be read or written.
fn filed<T>(done: std::io::Result<T>, what: &str) -> Result<T, String> {
    done.map_err(|error| format!("{what}: {error}"))
}

/// The text of the file at `path`.
fn text(path: &str) -> Result<String, String> {
    filed(
        std::fs::read_to_string(path),
        &format!("{path} could not be read"),
    )
}

/// `text` written to a new file at `path`, never over one that exists.
fn created(path: &str, text: &str) -> Result<(), String> {
    filed(
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .and_then(|mut file| file.write_all(text.as_bytes())),
        &format!("{path} could not be written"),
    )
}

/// `text` written to the file at `path`.
fn rewrite(path: &str, text: String) -> Result<(), String> {
    filed(
        std::fs::write(path, text),
        &format!("{path} could not be written"),
    )
}

fn read(path: &str) -> Result<Config, String> {
    loaded(path, &text(path)?)
}

/// The config `text` writes, as read from the file at `path`.
fn loaded(path: &str, text: &str) -> Result<Config, String> {
    let directory = Path::new(path).parent().unwrap_or(Path::new(""));
    let mut config = load_with(path, text, &Beside { directory }, &herald_compose::kinds())
        .map_err(|errors| errors.to_string())?;
    if let Some(options) = config.store.of::<herald_store_sqlite::StoreOptions>() {
        let mut options = options.clone();
        options.path = Some(options.file(directory).display().to_string());
        config.store = herald_config::ConfiguredStore::new(options);
    }
    Ok(config)
}

/// The files a config in `directory` names, each read from beside it.
struct Beside<'a> {
    directory: &'a Path,
}

impl Beside<'_> {
    /// The config's directory, `.` where its path names none.
    fn directory(&self) -> &Path {
        if self.directory.as_os_str().is_empty() {
            Path::new(".")
        } else {
            self.directory
        }
    }
}

impl herald_config::Files for Beside<'_> {
    fn read(&self, file: &str, within: Within<'_>) -> Result<String, Unread> {
        let failed = |error: std::io::Error| Unread::Failed(error.to_string());
        let base = match within {
            Within::Beside => self.directory(),
            Within::Allowed(directory) => Path::new(directory),
        };
        let base = std::fs::canonicalize(base).map_err(failed)?;
        let resolved = std::fs::canonicalize(self.directory().join(file)).map_err(failed)?;
        if !resolved.starts_with(&base) {
            return Err(Unread::Outside);
        }
        if !resolved.is_file() {
            return Err(Unread::Failed("it is no file".to_owned()));
        }
        std::fs::read_to_string(resolved).map_err(failed)
    }

    fn read_inside(&self, file: &str) -> Result<String, String> {
        self.read(file, Within::Beside)
            .map_err(|unread| match unread {
                Unread::Outside => format!(
                    "its links lead outside {}, the config's directory",
                    self.directory().display()
                ),
                Unread::Failed(why) => why,
            })
    }
}

/// The secret `herald token marker` names for the marker key it prints.
const MARKER_SECRET: &str = "HERALD_MARKER_SECRET";

/// A new marker secret printed once, or why none could be made.
fn marker(secret: Option<String>, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
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
    printed: Printed,
    out: &mut Logged<'_>,
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
        out.info("The config names no source to poll.");
    }
    let mut code = DONE;
    for swept in &swept {
        if let Sweep::Stopped(_)
        | Sweep::Polled {
            cursor: Remembered::Kept | Remembered::Failed(_),
            ..
        }
        | Sweep::Reported {
            cursor: Remembered::Kept | Remembered::Failed(_),
            ..
        } = swept.outcome
        {
            code = STOPPED;
        }
        out.all(polled_as(swept, printed));
    }
    Ok(code.max(pages::logged(&composed, out).await))
}

async fn housekeeping(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut Logged<'_>,
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
        out.info("No chore is due.");
    }
    let mut code = DONE;
    for chore in &kept.kept {
        if matches!(chore.outcome, Chored::Later(_)) {
            code = STOPPED;
        }
        out.told(&chored(chore));
    }
    for path in &kept.skipped {
        out.told(&skipped(path));
    }
    Ok(code)
}

#[cfg(test)]
mod tests;
