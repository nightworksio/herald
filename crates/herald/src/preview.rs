//! `herald preview`: each route's templates rendered as its destination
//! would show them, reading no secret and sending nothing (ADR-0095).

use std::io::Write;

use herald_config::{Config, Sample};
use herald_core::{Event, Kind, RouteId};
use herald_flows::{Against, Previewed, Sampling, preview, preview_group, preview_samples};
use herald_sources_forge::Forge;

use super::output::say;
use super::{DONE, MISTAKEN, STOPPED};

/// What `herald preview` was asked for, as given.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Asked<'a> {
    /// Only the route of this id.
    pub(crate) route: Option<&'a str>,
    /// Only this kind's samples.
    pub(crate) kind: Option<&'a str>,
    /// Only the sample of this name.
    pub(crate) sample: Option<&'a str>,
    /// Only the report of this name.
    pub(crate) report: Option<&'a str>,
    /// Each route that groups, rendering one group under this key.
    pub(crate) group: Option<&'a str>,
}

/// What `asked` names, each a route, kind, sample or report of `config`.
struct Named {
    route: Option<RouteId>,
    kind: Option<Kind>,
    sample: Option<Sample>,
}

/// Each route's templates rendered as `asked` says, against the release of
/// a forge's event file or the samples of each kind it takes, as its
/// destination would show them; or, under `group`, each grouping route's
/// as one group.
pub(super) fn previewing(
    config: &Config,
    asked: Asked<'_>,
    event_file: Option<(Forge, &str)>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let named = match named(config, asked) {
        Ok(named) => named,
        Err(message) => {
            say(err, &message);
            return MISTAKEN;
        }
    };
    let only = named.route.as_ref();
    let shown = match event_file {
        Some(file) => match event(file) {
            Ok(event) => match asked.group {
                Some(key) => preview_group(config, (key, std::slice::from_ref(&event)), only),
                None => preview(config, &event, only),
            },
            Err(message) => {
                say(err, &message);
                return MISTAKEN;
            }
        },
        None => preview_samples(
            config,
            Sampling {
                only,
                kind: named.kind.as_ref(),
                sample: named.sample,
                report: asked.report,
                group: asked.group,
            },
        ),
    };
    if shown.is_empty() {
        say(err, &unpreviewed(asked, event_file.is_some()));
        return MISTAKEN;
    }
    let mut code = DONE;
    for shown in shown {
        if !rendered(shown, out) {
            code = STOPPED;
        }
    }
    code
}

/// The route, kind and sample `asked` names, or why one names none of
/// `config`'s, which says only the names it has.
fn named(config: &Config, asked: Asked<'_>) -> Result<Named, String> {
    let routes: Vec<(&str, RouteId)> = config
        .routes
        .iter()
        .map(|route| (route.route.id.as_str(), route.route.id.clone()))
        .collect();
    let kinds: Vec<(&str, Kind)> = config
        .kinds
        .iter()
        .map(|spec| (spec.kind.as_str(), spec.kind.clone()))
        .collect();
    let reports: Vec<(&str, ())> = config
        .reports
        .keys()
        .map(|name| (name.as_str(), ()))
        .collect();
    picked(asked.report, ("--report", "report"), &reports)?;
    Ok(Named {
        route: picked(asked.route, ("--route", "route"), &routes)?,
        kind: picked(asked.kind, ("--kind", "kind"), &kinds)?,
        sample: picked(
            asked.sample,
            ("--sample", "sample"),
            &Sample::ALL.map(|sample| (sample.name(), sample)),
        )?,
    })
}

/// What of `known` the name `given` names, or why `flag` names no `what`:
/// it names none of `known`, which it names, or the config's none.
fn picked<T: Clone>(
    given: Option<&str>,
    (flag, what): (&str, &str),
    known: &[(&str, T)],
) -> Result<Option<T>, String> {
    let Some(given) = given else {
        return Ok(None);
    };
    if let Some((_, found)) = known.iter().find(|(name, _)| *name == given) {
        return Ok(Some(found.clone()));
    }
    let names: Vec<&str> = known.iter().map(|(name, _)| *name).collect();
    Err(if names.is_empty() {
        format!("{flag} names no {what}, and the config has none")
    } else {
        format!(
            "{flag} names no {what}; the {what}s are {}",
            names.join(", ")
        )
    })
}

/// Why nothing was previewed as `asked`, whose every name is known, of an
/// event file where `evented`: no route it names takes what it asks for.
fn unpreviewed(asked: Asked<'_>, evented: bool) -> String {
    let route = asked
        .route
        .map(|id| format!(" \"{id}\""))
        .unwrap_or_default();
    let grouping = if asked.group.is_some() {
        " that groups"
    } else {
        ""
    };
    let what = match (asked.report, asked.kind) {
        (Some(report), _) => format!("the report \"{report}\""),
        (None, Some(kind)) => format!("\"{kind}\" events"),
        (None, None) if evented => "the event".to_owned(),
        (None, None) => "events".to_owned(),
    };
    let sample = asked
        .sample
        .map(|sample| format!(" with a {sample} sample"))
        .unwrap_or_default();
    format!("nothing to preview: no route{route}{grouping} takes {what}{sample}")
}

/// The release a forge's event file publishes, or why the file gives none.
fn event((forge, path): (Forge, &str)) -> Result<Event, String> {
    let payload =
        std::fs::read(path).map_err(|error| format!("{path} could not be read: {error}"))?;
    forge
        .event(&payload, herald_core::PrereleaseRule::default())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("{path} publishes no release"))
}

/// Whether `shown` rendered, printed as its destination would show it or
/// with why its templates cannot render the event.
pub(crate) fn rendered(shown: Previewed, out: &mut dyn Write) -> bool {
    let to = format!("{} to {}", shown.route.as_str(), shown.destination.as_str());
    let head = match &shown.against {
        None => to,
        Some(Against::Sample(kind, sample)) => format!("{to}, sample {kind} ({})", sample.name()),
        Some(Against::Report(name)) => format!("{to}, report {name} (sample view)"),
        Some(Against::Group(members)) => format!("{to}, samples as one group ({members})"),
    };
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
