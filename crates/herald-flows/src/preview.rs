//! Each route's templates rendered as its destination would show them
//! (ADR-0006).

use herald_config::Config;
use herald_config::RouteConfig;
use herald_core::{Announcement, DestinationName, Event, Group, Marker, Marks, RouteId, Subject};

/// One route's templates rendered against an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Previewed {
    /// The route.
    pub route: RouteId,
    /// Its destination.
    pub destination: DestinationName,
    /// What the destination would show, or why the templates cannot render
    /// the event.
    pub rendered: Result<Announcement, String>,
}

/// Every route of `config`, or only `only`, rendering `event` as its
/// destination would show it (ADR-0006). No secret is resolved and nothing
/// is sent: the destination's dialect and limits come from the config, and
/// the markers are placeholders.
#[must_use]
pub fn preview(config: &Config, event: &Event, only: Option<&RouteId>) -> Vec<Previewed> {
    config
        .routes
        .iter()
        .filter(|route| only.is_none_or(|only| *only == route.route.id))
        .map(|route| previewed(config, route, event.into()))
        .collect()
}

/// Every route of `config` that groups, or only `only`, rendering `events`
/// as one group under `key`, its lead first, as its destination would show
/// it (ADR-0045).
#[must_use]
pub fn preview_group(
    config: &Config,
    (key, events): (&str, &[Event]),
    only: Option<&RouteId>,
) -> Vec<Previewed> {
    config
        .routes
        .iter()
        .filter(|route| only.is_none_or(|only| *only == route.route.id))
        .filter_map(|route| {
            let ordered = route.route.delivery.group.as_ref()?.ordered(events);
            let lead = *ordered.first()?;
            let group = Some(Group {
                key,
                events: &ordered,
            });
            Some(previewed(config, route, Subject { event: lead, group }))
        })
        .collect()
}

/// `route` of `config` rendering `subject` with placeholder markers.
fn previewed(config: &Config, route: &RouteConfig, subject: Subject<'_>) -> Previewed {
    let rendered = herald_core::announce(
        subject,
        &route.route,
        &config.templates,
        &route.profile,
        herald_core::itself::VERSION,
        Marks {
            route: Marker::of(&[0; 32]),
            announcement: Marker::of(&[0; 32]),
        },
        config.kinds.links(subject.event.kind()),
    )
    .map_err(|error| error.to_string());
    Previewed {
        route: route.route.id.clone(),
        destination: route.route.destination.clone(),
        rendered,
    }
}
