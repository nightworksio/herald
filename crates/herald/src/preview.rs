//! `herald preview`: each route's templates rendered as its destination
//! would show them.

use std::io::Write;

use herald_config::{Config, samples};
use herald_core::{Event, Kind, RouteId};
use herald_flows::{Previewed, preview};
use herald_sources_forge::Forge;

use super::output::say;
use super::{DONE, MISTAKEN, STOPPED};

/// Each route's templates rendered against the release of a forge's event
/// file, or a sample release, as its destination would show them.
pub(super) fn previewing(
    config: &Config,
    route: Option<&str>,
    event_file: Option<(Forge, &str)>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let events = match events(config, event_file) {
        Ok(events) => events,
        Err(message) => {
            say(err, &message);
            return MISTAKEN;
        }
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
            if !rendered(shown, out) {
                code = STOPPED;
            }
        }
    }
    code
}

/// The release a forge's event file publishes, or else a sample release, or
/// why the file gives none.
fn events(config: &Config, event_file: Option<(Forge, &str)>) -> Result<Vec<Event>, String> {
    let Some((forge, path)) = event_file else {
        return Ok(config
            .kinds
            .get(&Kind::RELEASE)
            .map(samples)
            .unwrap_or_default()
            .into_iter()
            .take(1)
            .collect());
    };
    let payload =
        std::fs::read(path).map_err(|error| format!("{path} could not be read: {error}"))?;
    match forge.event(&payload).map_err(|error| error.to_string())? {
        Some(event) => Ok(vec![event]),
        None => Err(format!("{path} publishes no release")),
    }
}

/// Whether `shown` rendered, printed as its destination would show it or
/// with why its templates cannot render the event.
fn rendered(shown: Previewed, out: &mut dyn Write) -> bool {
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
            true
        }
        Err(reason) => {
            say(
                out,
                &format!("{head}: the templates cannot render this event: {reason}"),
            );
            false
        }
    }
}
