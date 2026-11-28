//! Each route's templates rendered as its destination would show them
//! (ADR-0006, ADR-0095).

use herald_config::{Config, RouteConfig, SAMPLED, Sample, sample_boards, sample_origin, sampled};
use herald_core::{
    Announcement, Board, DestinationName, Event, Group, Kind, Marker, Marks, RouteId, Showing,
    Subject, Templates, report_event,
};

mod wire;

pub use wire::{DRY_ID, FED, FOLLOWED, PUSHED, Recording, UNREACHED, Wire, wired};

/// One route's templates rendered against an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Previewed {
    /// The route.
    pub route: RouteId,
    /// Its destination.
    pub destination: DestinationName,
    /// The sample it was rendered against, where it was one.
    pub against: Option<Against>,
    /// What the destination would show, or why the templates cannot render
    /// the event.
    pub rendered: Result<Announcement, String>,
}

/// The sample a route was rendered against (ADR-0095).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Against {
    /// One of a kind's samples.
    Sample(Kind, Sample),
    /// The report of this name, showing a sample view.
    Report(String),
    /// The samples of every kind it takes, as one group of this many.
    Group(usize),
}

/// What [`preview_samples`] renders each route against: the samples of
/// each kind it takes, as each of these narrows them.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sampling<'a> {
    /// Only this route.
    pub only: Option<&'a RouteId>,
    /// Only this kind.
    pub kind: Option<&'a Kind>,
    /// Only this sample of each kind.
    pub sample: Option<Sample>,
    /// Only the report of this name.
    pub report: Option<&'a str>,
    /// Every sample, or only `sample`, as one group under this key, on each
    /// route that groups (ADR-0045).
    pub group: Option<&'a str>,
}

/// Every route of `config` that takes `event` by everything but its
/// source, or only `only`, rendering it as its destination would show it
/// (ADR-0006). No secret is resolved and nothing is sent: the
/// destination's dialect and limits come from the config, and the markers
/// are placeholders.
#[must_use]
pub fn preview(config: &Config, event: &Event, only: Option<&RouteId>) -> Vec<Previewed> {
    routes(config, only)
        .filter(|route| takes(route, event))
        .map(|route| previewed(config, route, event.into()))
        .collect()
}

/// Every route of `config` that groups, or only `only`, rendering those of
/// `events` it takes by everything but their source as one group under
/// `key`, its lead first, as its destination would show it (ADR-0045).
#[must_use]
pub fn preview_group(
    config: &Config,
    (key, events): (&str, &[Event]),
    only: Option<&RouteId>,
) -> Vec<Previewed> {
    routes(config, only)
        .filter_map(|route| {
            let taken: Vec<Event> = events
                .iter()
                .filter(|event| takes(route, event))
                .cloned()
                .collect();
            as_group(config, route, (key, &taken))
        })
        .collect()
}

/// Whether `route` takes `event` by everything but its source.
fn takes(route: &RouteConfig, event: &Event) -> bool {
    route.taken.contains(event.kind()) && route.route.filter.takes_event(event)
}

/// Every route of `config` rendering the samples of each kind it takes as
/// `sampling` says, as its destination would show them (ADR-0095): a
/// report's against a sample view, as the report shows it.
#[must_use]
pub fn preview_samples(config: &Config, sampling: Sampling<'_>) -> Vec<Previewed> {
    let boards = sample_boards();
    routes(config, sampling.only)
        .flat_map(|route| match sampling.group {
            Some(key) => grouped(config, route, sampling, key)
                .into_iter()
                .collect::<Vec<_>>(),
            None => each(config, route, sampling, &boards[0]),
        })
        .collect()
}

/// Every route of `config`, or only `only`.
fn routes<'a>(config: &'a Config, only: Option<&RouteId>) -> impl Iterator<Item = &'a RouteConfig> {
    config
        .routes
        .iter()
        .filter(move |route| only.is_none_or(|only| *only == route.route.id))
}

/// `route` rendering each sample `sampling` names of each kind it takes,
/// and each report it takes showing `board`.
fn each(
    config: &Config,
    route: &RouteConfig,
    sampling: Sampling<'_>,
    board: &Board,
) -> Vec<Previewed> {
    let events = samples(config, route, sampling)
        .into_iter()
        .map(|(kind, sample, event)| Previewed {
            against: Some(Against::Sample(kind, sample)),
            ..previewed(config, route, (&event).into())
        });
    events
        .chain(reports(config, route, sampling, board))
        .collect()
}

/// The samples `sampling` names of each kind but a report that `route`
/// takes, each with its kind and which sample it is, from the sample
/// origin.
fn samples(
    config: &Config,
    route: &RouteConfig,
    sampling: Sampling<'_>,
) -> Vec<(Kind, Sample, Event)> {
    if sampling.report.is_some() {
        return Vec::new();
    }
    route
        .taken
        .iter()
        .filter(|kind| **kind != Kind::REPORT && sampling.kind.is_none_or(|only| only == *kind))
        .filter_map(|kind| config.kinds.get(kind))
        .flat_map(|spec| {
            sampled(spec)
                .into_iter()
                .map(|(sample, event)| (spec.kind.clone(), sample, event))
        })
        .filter(|(_, sample, _)| sampling.sample.is_none_or(|only| only == *sample))
        .map(|(kind, sample, event)| (kind, sample, event.from_origin(sample_origin())))
        .collect()
}

/// `route` rendering each report it takes, or only the one `sampling`
/// names, showing `board` as the report's view, where `sampling` asks for
/// reports and for no sample but the plain one.
fn reports(
    config: &Config,
    route: &RouteConfig,
    sampling: Sampling<'_>,
    board: &Board,
) -> Vec<Previewed> {
    let asked = sampling.sample.unwrap_or_default() == Sample::Plain
        && sampling.kind.is_none_or(|kind| *kind == Kind::REPORT);
    if !asked || !route.taken.contains(&Kind::REPORT) {
        return Vec::new();
    }
    route
        .route
        .filter
        .sources
        .iter()
        .filter_map(|source| config.reports.get_key_value(source.as_str()))
        .filter(|(name, _)| sampling.report.is_none_or(|only| only == name.as_str()))
        .filter_map(|(name, report)| {
            let event = report_event((name, &report.view), board, SAMPLED, route.living()).ok()?;
            let subject = Subject {
                event: &event,
                group: None,
                view: Some(Showing {
                    view: &report.view,
                    board,
                }),
            };
            Some(Previewed {
                against: Some(Against::Report(name.clone())),
                ..previewed(config, route, subject)
            })
        })
        .collect()
}

/// `route`, where it groups, rendering the samples `sampling` names of each
/// kind it takes as one group under `key`.
fn grouped(
    config: &Config,
    route: &RouteConfig,
    sampling: Sampling<'_>,
    key: &str,
) -> Option<Previewed> {
    let events: Vec<Event> = samples(config, route, sampling)
        .into_iter()
        .map(|(_, _, event)| event)
        .collect();
    let previewed = as_group(config, route, (key, &events))?;
    Some(Previewed {
        against: Some(Against::Group(events.len())),
        ..previewed
    })
}

/// `route`, where it groups, rendering `events` as one group under `key`,
/// its lead first.
fn as_group(
    config: &Config,
    route: &RouteConfig,
    (key, events): (&str, &[Event]),
) -> Option<Previewed> {
    let ordered = route.route.delivery.group.as_ref()?.ordered(events);
    let lead = *ordered.first()?;
    let group = Some(Group {
        key,
        events: &ordered,
    });
    Some(previewed(
        config,
        route,
        Subject {
            event: lead,
            group,
            view: None,
        },
    ))
}

/// `route` of `config` rendering `subject` with placeholder markers.
fn previewed(config: &Config, route: &RouteConfig, subject: Subject<'_>) -> Previewed {
    rendered(config, route, &config.templates, subject)
}

/// `route` of `config` rendering `subject` through `templates` with
/// placeholder markers.
pub(crate) fn rendered(
    config: &Config,
    route: &RouteConfig,
    templates: &Templates,
    subject: Subject<'_>,
) -> Previewed {
    let rendered = herald_core::announce(
        subject,
        &route.route,
        templates,
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
        against: None,
        rendered,
    }
}
