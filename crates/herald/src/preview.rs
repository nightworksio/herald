//! `herald preview`: each route's templates rendered as its destination
//! would show them.

use std::io::Write;

use herald_config::{Config, samples_from};
use herald_core::{Event, Kind, RouteId};
use herald_flows::{Previewed, preview, preview_group};
use herald_sources_forge::Forge;

use super::output::say;
use super::{DONE, MISTAKEN, STOPPED};

/// Each route's templates rendered against the release of a forge's event
/// file, or a sample release, as its destination would show them; or, under
/// `group`, each grouping route's against the file's release or every sample
/// release as one group.
pub(super) fn previewing(
    config: &Config,
    route: Option<&str>,
    event_file: Option<(Forge, &str)>,
    group: Option<&str>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let events = match events(config, event_file, group.is_some()) {
        Ok(events) => events,
        Err(message) => {
            say(err, &message);
            return MISTAKEN;
        }
    };
    let only = route.map(RouteId::new);
    let shown: Vec<Vec<Previewed>> = match group {
        Some(key) => vec![preview_group(config, (key, &events), only.as_ref())],
        None => events
            .iter()
            .map(|event| preview(config, event, only.as_ref()))
            .collect(),
    };
    let mut code = DONE;
    for previewed in shown {
        if previewed.is_empty() {
            say(err, &unpreviewed(route, group.is_some()));
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

/// Why nothing was previewed: no route has the id `route`, or none that
/// groups where `grouped`.
fn unpreviewed(route: Option<&str>, grouped: bool) -> String {
    match (route, grouped) {
        (Some(id), true) => format!("no route that groups has the id \"{id}\""),
        (None, true) => "no route groups".to_owned(),
        (id, false) => format!("no route has the id \"{}\"", id.unwrap_or_default()),
    }
}

/// The release a forge's event file publishes, or else a sample release,
/// every sample release where `every`, or why the file gives none.
fn events(
    config: &Config,
    event_file: Option<(Forge, &str)>,
    every: bool,
) -> Result<Vec<Event>, String> {
    let Some((forge, path)) = event_file else {
        return Ok(config
            .kinds
            .get(&Kind::RELEASE)
            .map(|spec| samples_from(spec, true))
            .unwrap_or_default()
            .into_iter()
            .take(if every { usize::MAX } else { 1 })
            .collect());
    };
    let payload =
        std::fs::read(path).map_err(|error| format!("{path} could not be read: {error}"))?;
    match forge
        .event(&payload, herald_core::PrereleaseRule::default())
        .map_err(|error| error.to_string())?
    {
        Some(event) => Ok(vec![event]),
        None => Err(format!("{path} publishes no release")),
    }
}

/// Whether `shown` rendered, printed as its destination would show it or
/// with why its templates cannot render the event.
pub(crate) fn rendered(shown: Previewed, out: &mut dyn Write) -> bool {
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
