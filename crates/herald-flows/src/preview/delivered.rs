//! A forge's delivery read by the config's sources, as their webhooks read
//! it, and each route rendered against the events they map it to
//! (ADR-0095).

use herald_config::{Config, RouteConfig};
use herald_core::{Event, RouteId, SourceName};
use herald_kinds::Hooked;

use super::{Previewed, as_group, previewed, routes};

/// What a source of the config makes of a delivery that is no event it
/// announces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unmapped {
    /// It reads the delivery as nothing it announces, for this reason.
    Nothing(&'static str),
    /// It cannot read the delivery: not the event it is named, or of
    /// another repository than the source takes.
    Unread,
    /// It reads the delivery as a tag or package a poll reads in full, or
    /// an owner's repositories to list again, which preview does not do.
    Polled,
}

/// An event, with the source that maps it.
pub type Sourced = (SourceName, Event);

/// A delivery's events, each with the source of `config` that maps it, as
/// that source's webhook reads `headers` and `payload`; or what each source
/// made of it, where none maps it to an event. An owner's member's event
/// is seen only as the member's visibility lets it be seen.
///
/// # Errors
///
/// What each source made of a delivery none maps to an event.
pub fn delivered(
    config: &Config,
    (headers, payload): (&[(&str, &str)], &[u8]),
) -> Result<Vec<Sourced>, Vec<(SourceName, Unmapped)>> {
    let mut events = Vec::new();
    let mut unmapped = Vec::new();
    for (name, source) in &config.sources {
        match source.hooked(headers, payload) {
            Ok(Hooked::Event(event)) => events.push((name.clone(), event)),
            Ok(Hooked::Member {
                event, visibility, ..
            }) => events.push((
                name.clone(),
                visibility.into_iter().fold(event, Event::seen_as),
            )),
            Ok(Hooked::Poll | Hooked::Relist) => unmapped.push((name.clone(), Unmapped::Polled)),
            Ok(Hooked::Nothing(why)) => unmapped.push((name.clone(), Unmapped::Nothing(why))),
            Err(_) => unmapped.push((name.clone(), Unmapped::Unread)),
        }
    }
    if events.is_empty() {
        Err(unmapped)
    } else {
        Ok(events)
    }
}

/// Every route of `config`, or only `only`, rendering the first of
/// `events` it takes from its source, as its destination would show it.
#[must_use]
pub fn preview_delivered(
    config: &Config,
    events: &[Sourced],
    only: Option<&RouteId>,
) -> Vec<Previewed> {
    routes(config, only)
        .filter_map(|route| {
            let (_, event) = events
                .iter()
                .find(|(source, event)| takes(route, source, event))?;
            Some(previewed(config, route, event.into()))
        })
        .collect()
}

/// Every route of `config` that groups, or only `only`, rendering those of
/// `events` it takes from their sources as one group under `key`, its lead
/// first (ADR-0045).
#[must_use]
pub fn preview_delivered_group(
    config: &Config,
    (key, events): (&str, &[Sourced]),
    only: Option<&RouteId>,
) -> Vec<Previewed> {
    routes(config, only)
        .filter_map(|route| {
            let taken: Vec<Event> = events
                .iter()
                .filter(|(source, event)| takes(route, source, event))
                .map(|(_, event)| event.clone())
                .collect();
            as_group(config, route, (key, &taken))
        })
        .collect()
}

/// Whether `route` takes `event` from `source`.
fn takes(route: &RouteConfig, source: &SourceName, event: &Event) -> bool {
    route.taken.contains(event.kind()) && route.route.filter.takes(source, event)
}
