//! The HTTP API's apps from the CLI (ADR-0003, ADR-0007): a new token for
//! an app, and an app's event announced to the routes the app may announce
//! to.

use std::collections::BTreeMap;
use std::io::Write;

use herald_config::Config;
use herald_core::{SourceName, digest};
use herald_native::SystemClock;
use herald_ports::Clock as _;
use tokio::io::{AsyncRead, AsyncReadExt as _};

use super::output::say;
use super::{Connect, DONE, Routed, STOPPED, announcing};

/// An app's event to announce, as the CLI names it.
pub(crate) struct Written<'a> {
    /// The app the event is of.
    pub(crate) app: &'a str,
    /// The file the event is read from, or `-` for standard input.
    pub(crate) event: &'a str,
    /// Whether to print the requests and send nothing.
    pub(crate) dry_run: bool,
}

/// A new token for `app` printed once, with the line herald.toml keeps for
/// it, or why none could be made.
pub(crate) fn token(
    app: &str,
    token: Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let Some(token) = token else {
        say(
            err,
            "the system's random generator failed, so no token was made",
        );
        return STOPPED;
    };
    say(out, &format!("The app {app}'s token: {token}"));
    say(
        out,
        &format!(
            "Give it to the app as its bearer token, and keep its hash in herald.toml under [apps.{app}]:"
        ),
    );
    say(out, &format!("tokens = [\"sha256:{}\"]", digest(&token)));
    say(
        out,
        "herald prints the token only this once, and keeps nothing but its hash.",
    );
    DONE
}

/// The event `written` names announced to every route its app may announce
/// to that takes it.
pub(crate) async fn announcing_event(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    written: Written<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let app = written.app;
    let Some(named) = config.apps.get(app) else {
        return Err(format!(
            "no app in the config is named \"{app}\"; add [apps.{app}]"
        ));
    };
    let bytes = read(written.event, &mut tokio::io::stdin()).await?;
    let event = herald_sources_app::event(app, &bytes, SystemClock.now())
        .map_err(|unread| unread.to_string())?;
    if !named
        .projects
        .iter()
        .any(|project| project == event.project())
    {
        return Err(format!(
            "the app \"{app}\" may not announce the project \"{}\"; add it to [apps.{app}] projects",
            event.project()
        ));
    }
    let source = SourceName::new(app);
    let routed = Routed {
        sources: &[&source],
        within: Some(&named.routes),
        event: &event,
    };
    announcing(config, vars, connect, routed, written.dry_run, out).await
}

/// The bytes of the event file `event` names, or of `stdin` when it is `-`.
pub(crate) async fn read(
    event: &str,
    stdin: &mut (dyn AsyncRead + Send + Unpin),
) -> Result<Vec<u8>, String> {
    if event == "-" {
        let mut bytes = Vec::new();
        stdin
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| format!("standard input could not be read: {error}"))?;
        Ok(bytes)
    } else {
        tokio::fs::read(event)
            .await
            .map_err(|error| format!("{event} could not be read: {error}"))
    }
}
