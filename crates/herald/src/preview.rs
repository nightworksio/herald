//! `herald preview`: each route's templates rendered as its destination
//! would show them, reading no secret and sending nothing (ADR-0095).

use std::io::{Read, Write};

use herald_config::{Config, Sample};
use herald_core::{Announcement, Event, Kind, RouteId};
use herald_flows::{
    Against, Previewed, Publisher, Sampling, Served, Wire, enveloped, preview_delivered,
    preview_delivered_group, preview_samples, wired,
};
use herald_ports::Request;
use serde_json::{Map, Value, json};

mod event;

use event::EventFile;

use super::cli::{PreviewFormat, RunForge};
use super::output::{say, say_json};
use super::{DONE, MISTAKEN, STOPPED};

/// The flags `herald preview` takes.
#[derive(Debug, clap::Args)]
pub(crate) struct Flags {
    /// Only this route.
    #[arg(long)]
    pub(crate) route: Option<String>,
    /// Only this kind's samples.
    #[arg(long, conflicts_with = "event")]
    pub(crate) kind: Option<String>,
    /// Only this sample of each kind.
    #[arg(long, conflicts_with = "event")]
    pub(crate) sample: Option<String>,
    /// Only this report, showing a sample view.
    #[arg(long, conflicts_with = "event")]
    pub(crate) report: Option<String>,
    /// A forge's event file, as its webhook delivers it, `-` for
    /// standard input, read by each source of the config as its
    /// webhook reads it, in place of a sample event.
    #[arg(long)]
    pub(crate) event: Option<String>,
    /// The forge whose event the file is.
    #[arg(long, value_enum, default_value_t = RunForge::Github)]
    pub(crate) from: RunForge,
    /// The event's name, as the forge's event header names it
    /// (`pull_request`, `Merge Request Hook`); its release event where
    /// absent.
    #[arg(long, requires = "event")]
    pub(crate) event_name: Option<String>,
    /// Say this output is private, so an event of a private or internal
    /// repository is shown; without it, or `[cli] private`, each route
    /// it reaches says "(private)" alone (ADR-0080).
    #[arg(long, requires = "event")]
    pub(crate) private: bool,
    /// Render each route that groups against the file's event, or
    /// every sample of each kind it takes, as one group under this key.
    #[arg(long)]
    pub(crate) group: Option<String>,
    /// What is printed of each route's message.
    #[arg(long, value_enum, default_value_t = PreviewFormat::Text)]
    pub(crate) format: PreviewFormat,
}

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

/// Each route's templates rendered as `flags` say, standard input read
/// from `input` where `--event -` names it (ADR-0095).
pub(super) async fn previewing(
    config: &Config,
    flags: &Flags,
    input: &mut dyn Read,
    (out, err): (&mut dyn Write, &mut dyn Write),
) -> u8 {
    let asked = Asked {
        route: flags.route.as_deref(),
        kind: flags.kind.as_deref(),
        sample: flags.sample.as_deref(),
        report: flags.report.as_deref(),
        group: flags.group.as_deref(),
    };
    let file = flags.event.as_deref().map(|path| EventFile {
        forge: flags.from.forge(),
        path,
        name: flags.event_name.as_deref(),
        printed: herald_core::Printed::of(flags.private, config.cli.private),
    });
    shown_as(config, (asked, flags.format), (file, input), (out, err)).await
}

/// Each route's templates rendered as `asked` says, against the events the
/// config's sources map a forge's event file to, standard input read from
/// `input` where it names it, or the samples of each kind it takes, as its
/// destination would show them; or, under `group`, each grouping route's
/// as one group: printed in `format`, each route an event the output
/// withholds reaches saying "(private)" alone.
async fn shown_as(
    config: &Config,
    (asked, format): (Asked<'_>, PreviewFormat),
    (event_file, input): (Option<EventFile<'_>>, &mut dyn Read),
    (out, err): (&mut dyn Write, &mut dyn Write),
) -> u8 {
    let named = match named(config, asked) {
        Ok(named) => named,
        Err(message) => {
            say(err, &message);
            return MISTAKEN;
        }
    };
    let only = named.route.as_ref();
    let (shown, withheld) = match event_file {
        Some(file) => match event::events(config, file, input) {
            Ok(events) => (
                match asked.group {
                    Some(key) => preview_delivered_group(config, (key, &events), only),
                    None => preview_delivered(config, &events, only),
                },
                event::withheld(&events, file.printed),
            ),
            Err(message) => {
                for line in message.lines() {
                    say(err, line);
                }
                return MISTAKEN;
            }
        },
        None => (
            preview_samples(
                config,
                Sampling {
                    only,
                    kind: named.kind.as_ref(),
                    sample: named.sample,
                    report: asked.report,
                    group: asked.group,
                },
            ),
            false,
        ),
    };
    if shown.is_empty() {
        say(err, &unpreviewed(asked, event_file.is_some()));
        return MISTAKEN;
    }
    let code = if shown
        .iter()
        .all(|shown| shown.envelopes || shown.rendered.is_ok())
    {
        DONE
    } else {
        STOPPED
    };
    if withheld {
        private(&shown, format, out);
        return code;
    }
    match format {
        PreviewFormat::Text => {
            for shown in &shown {
                rendered(shown, out);
            }
        }
        PreviewFormat::Wire => {
            for shown in &shown {
                match wire_of(config, shown).await {
                    Ok((wire, _)) => printed_wire(shown, &wire, out),
                    Err(reason) => unrendered(shown, reason, out),
                }
            }
        }
        PreviewFormat::Json => {
            let mut all = Vec::new();
            for shown in &shown {
                all.push(as_json(config, shown).await);
            }
            say_json(out, &Value::Array(all).to_string());
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

/// Each of `shown` printed in `format` as a route an event the output
/// withholds reaches: its head and "(private)" alone.
fn private(shown: &[Previewed], format: PreviewFormat, out: &mut dyn Write) {
    match format {
        PreviewFormat::Text | PreviewFormat::Wire => {
            for shown in shown {
                say(out, &format!("{}: {}", head(shown), Event::PRIVATELY_NAMED));
            }
        }
        PreviewFormat::Json => {
            let all: Vec<Value> = shown
                .iter()
                .map(|shown| {
                    json!({
                        "route": shown.route.as_str(),
                        "destination": shown.destination.as_str(),
                        "against": Value::Null,
                        "private": true,
                    })
                })
                .collect();
            say_json(out, &Value::Array(all).to_string());
        }
    }
}

/// What `shown` names: its route, its destination and what it rendered.
fn head(shown: &Previewed) -> String {
    let to = format!("{} to {}", shown.route.as_str(), shown.destination.as_str());
    match &shown.against {
        None => to,
        Some(Against::Sample(kind, sample)) => format!("{to}, sample {kind} ({})", sample.name()),
        Some(Against::Report(name)) => format!("{to}, report {name} (sample view)"),
        Some(Against::Group(members)) => format!("{to}, samples as one group ({members})"),
    }
}

/// Why the templates cannot render `shown`'s event, printed after its head.
fn unrendered(shown: &Previewed, reason: &str, out: &mut dyn Write) {
    say(
        out,
        &format!(
            "{}: the templates cannot render this event: {reason}",
            head(shown)
        ),
    );
}

/// `shown` printed as its destination would show it, or with why its
/// templates cannot render the event; a route that publishes envelopes
/// says so.
pub(crate) fn rendered(shown: &Previewed, out: &mut dyn Write) {
    if shown.envelopes {
        return say(out, &format!("{}: {ENVELOPES}", head(shown)));
    }
    match &shown.rendered {
        Ok(announcement) => {
            say(out, &format!("{}:", head(shown)));
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
        Err(reason) => unrendered(shown, reason, out),
    }
}

/// What `shown`'s route sends or publishes, and what it rendered where it
/// renders through a template; or why its templates rendered nothing.
async fn wire_of<'a>(
    config: &Config,
    shown: &'a Previewed,
) -> Result<(Wire, Option<&'a Announcement>), &'a str> {
    let event = shown.event.as_ref().ok_or(UNEVENTED)?;
    if shown.envelopes {
        return Ok((enveloped(config, &shown.route, event), None));
    }
    let announcement = shown.rendered.as_ref().map_err(String::as_str)?;
    let wire = wired(config, &shown.route, (announcement, event)).await;
    Ok((wire, Some(announcement)))
}

/// What a route that publishes each event as an envelope shows as text.
const ENVELOPES: &str =
    "it publishes each event as an envelope rather than a message; --format wire shows it";

/// Why a rendered message with no event shows no request.
const UNEVENTED: &str = "it renders no event";

/// What `served` says of the document it holds: who publishes it, and
/// what proves it, by name.
fn served_as(served: &Served) -> String {
    let proof = served.proof.named();
    match &served.by {
        Publisher::Actor(actor) => format!(
            "served, not sent: the actor {actor} publishes this document in its outbox and delivers it to each follower's inbox in a request signed by {proof}"
        ),
        Publisher::Feed => {
            format!("served, not sent: herald's feed lists this envelope, proved by {proof}")
        }
        Publisher::Peer(peer) => format!(
            "served, not sent: herald pushes this envelope to the peer {peer}'s inbox, proved by {proof}"
        ),
    }
}

/// `shown` printed as the requests `wire` holds, each step named and each
/// request as it is sent, its secrets by name; or the document it serves;
/// or with why it shows none.
fn printed_wire(shown: &Previewed, wire: &Wire, out: &mut dyn Write) {
    let steps = match wire {
        Wire::Unsent(why) => return say(out, &format!("{}: {why}", head(shown))),
        Wire::Served(served) => {
            say(out, &format!("{}:", head(shown)));
            say(out, &format!("  {}:", served_as(served)));
            let pretty = serde_json::to_string_pretty(&document(served)).unwrap_or_default();
            for line in pretty.lines() {
                say(out, &format!("    {line}"));
            }
            return;
        }
        Wire::Sent(steps) => steps,
    };
    say(out, &format!("{}:", head(shown)));
    for (step, requests) in steps {
        let named = herald_compose::told::steps(std::slice::from_ref(step));
        if requests.is_empty() {
            say(out, &format!("  {named}: no request"));
            continue;
        }
        say(out, &format!("  {named}:"));
        for request in requests {
            for line in request.with_body().to_string().lines() {
                say(out, &format!("    {line}"));
            }
        }
    }
}

/// `shown` as JSON: what it renders against, its text, and the requests its
/// first post sends; or why its templates cannot render the event.
async fn as_json(config: &Config, shown: &Previewed) -> Value {
    let against = match &shown.against {
        None => Value::Null,
        Some(Against::Sample(kind, sample)) => {
            json!({ "kind": kind.as_str(), "sample": sample.name() })
        }
        Some(Against::Report(name)) => json!({ "report": name }),
        Some(Against::Group(members)) => json!({ "group": members }),
    };
    let mut fields = Map::new();
    fields.insert("route".to_owned(), json!(shown.route.as_str()));
    fields.insert("destination".to_owned(), json!(shown.destination.as_str()));
    fields.insert("against".to_owned(), against);
    match wire_of(config, shown).await {
        Ok((wire, announcement)) => {
            fields.extend(text_json(announcement, &wire));
        }
        Err(reason) => {
            fields.insert("error".to_owned(), json!(reason));
        }
    }
    Value::Object(fields)
}

/// The text of `announcement` and the requests `wire` holds, as JSON
/// fields.
fn text_json(announcement: Option<&Announcement>, wire: &Wire) -> Map<String, Value> {
    let (steps, unsent, served) = match wire {
        Wire::Sent(steps) => (
            steps
                .iter()
                .map(|(step, requests)| {
                    json!({
                        "step": step,
                        "requests": requests.iter().map(request_json).collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>(),
            Value::Null,
            Value::Null,
        ),
        Wire::Served(served) => (Vec::new(), Value::Null, served_json(served)),
        Wire::Unsent(why) => (Vec::new(), json!(why), Value::Null),
    };
    let parts: Option<Vec<&str>> = announcement.map(|announcement| {
        announcement
            .parts
            .iter()
            .map(|part| part.body.as_str())
            .collect()
    });
    Map::from_iter([
        (
            "title".to_owned(),
            json!(announcement.map(|announcement| announcement.title.plain())),
        ),
        (
            "thread".to_owned(),
            json!(announcement.and_then(|announcement| announcement.thread.as_ref())),
        ),
        ("parts".to_owned(), json!(parts)),
        ("steps".to_owned(), json!(steps)),
        ("unsent".to_owned(), unsent),
        ("served".to_owned(), served),
    ])
}

/// `served` as JSON: who publishes it, its name, what proves it, by name,
/// and the document.
fn served_json(served: &Served) -> Value {
    let (by, name) = match &served.by {
        Publisher::Actor(actor) => ("actor", Some(actor.as_str())),
        Publisher::Feed => ("feed", None),
        Publisher::Peer(peer) => ("peer", Some(peer.as_str())),
    };
    json!({
        "by": by,
        "name": name,
        "proof": served.proof.named(),
        "document": document(served),
    })
}

/// The document `served` holds, as the JSON it is.
fn document(served: &Served) -> Value {
    serde_json::from_str(&served.document).unwrap_or_default()
}

/// `request` as JSON, its secrets by name: its body as the JSON it holds,
/// as a dry run shows it where it holds none.
pub(crate) fn request_json(request: &Request) -> Value {
    let body = request.shown_body().map_or(Value::Null, |shown| {
        serde_json::from_str(&shown).unwrap_or_else(|_| json!(shown))
    });
    json!({
        "method": request.method.as_str(),
        "url": request.url.redacted(),
        "headers": request
            .headers
            .iter()
            .map(|(name, value)| json!([name, value.redacted()]))
            .collect::<Vec<_>>(),
        "body": body,
    })
}
