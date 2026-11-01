use herald_config::{Config, Destination, RouteOptions};
use herald_core::{Announcement, DestinationName, Event, Marker, Marks, RouteId};
use herald_dest_discord::profile;

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
    let placeholder = || Marks {
        route: Marker::of(&[0; 32]),
        announcement: Marker::of(&[0; 32]),
    };
    config
        .routes
        .iter()
        .filter(|route| only.is_none_or(|only| *only == route.route.id))
        .map(|route| {
            let bot = config
                .destinations
                .get(&route.route.destination)
                .is_some_and(|Destination::Discord(options)| options.token.is_some());
            let RouteOptions::Discord(placed) = &route.options;
            let rendered = herald_core::announce(
                event,
                &route.route,
                &config.templates,
                &profile(bot, placed.forum),
                env!("CARGO_PKG_VERSION"),
                placeholder(),
            )
            .map_err(|error| error.to_string());
            Previewed {
                route: route.route.id.clone(),
                destination: route.route.destination.clone(),
                rendered,
            }
        })
        .collect()
}
