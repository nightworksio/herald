//! A forge's event file, read by the config's sources as their webhooks
//! read a delivery, for `herald preview --event` (ADR-0095).

use std::io::Read;

use herald_config::Config;
use herald_core::{Printed, SourceName};
use herald_flows::{Sourced, Unmapped, delivered};
use herald_sources_forge::Forge;

/// The file `herald preview --event` names, and how it is read.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EventFile<'a> {
    /// The forge whose delivery it is.
    pub(crate) forge: Forge,
    /// Its path, `-` for standard input.
    pub(crate) path: &'a str,
    /// The event's name as the forge's header names it; its release
    /// event's where absent.
    pub(crate) name: Option<&'a str>,
    /// How private the output is.
    pub(crate) printed: Printed,
}

/// The path that names standard input.
const STANDARD_INPUT: &str = "-";

/// Why a source cannot read the file.
const UNREAD: &str =
    "it cannot read it: it is not the event --event-name names, or not of a repository it takes";

/// Why a source leaves the file to a poll.
const POLLED: &str = "it reads it as a tag, a package or an owner's repositories, which a poll reads in full and preview does not";

/// The events the sources of `config` map `file` to, each with its source,
/// reading standard input from `input`; or why none does, naming neither
/// the path nor what the file holds.
pub(crate) fn events(
    config: &Config,
    file: EventFile<'_>,
    input: &mut dyn Read,
) -> Result<Vec<Sourced>, String> {
    let payload = if file.path == STANDARD_INPUT {
        let mut payload = Vec::new();
        input.read_to_end(&mut payload).map(|_| payload)
    } else {
        std::fs::read(file.path)
    }
    .map_err(|error| format!("the event file could not be read: {error}"))?;
    if let Err(error) = serde_json::from_slice::<serde::de::IgnoredAny>(&payload) {
        return Err(herald_core::unparsed(
            "the event file",
            error.line(),
            Some(error.column()),
        ));
    }
    let header = file.forge.event_header();
    let name = file.name.unwrap_or_else(|| file.forge.release_event());
    delivered(config, (&[(header, name)], &payload)).map_err(|unmapped| unread(&unmapped))
}

/// Whether the output withholds any of `events`, as `printed` says.
pub(crate) fn withheld(events: &[Sourced], printed: Printed) -> bool {
    events.iter().any(|(_, event)| {
        event
            .bounded_by()
            .is_some_and(|visibility| !printed.prints(visibility))
    })
}

/// Why no source maps the file to an event, from what each made of it.
fn unread(unmapped: &[(SourceName, Unmapped)]) -> String {
    if unmapped.is_empty() {
        return "nothing to preview: the config has no source to read the event file".to_owned();
    }
    let said = unmapped.iter().map(|(source, why)| {
        let why = match why {
            Unmapped::Nothing(why) => why,
            Unmapped::Unread => UNREAD,
            Unmapped::Polled => POLLED,
        };
        format!("  source \"{}\": {why}", source.as_str())
    });
    std::iter::once(
        "nothing to preview: no source of the config reads the event file as an event it announces"
            .to_owned(),
    )
    .chain(said)
    .collect::<Vec<_>>()
    .join("\n")
}
