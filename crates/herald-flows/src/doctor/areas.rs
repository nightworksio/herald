//! What doctor says of each route narrowing by areas: where the paths its
//! events' areas are those of are read, what that costs, and when an event
//! has none (ADR-0072).

use herald_config::Config;
use herald_core::{AreaPaths, SourceName};

/// What doctor says of an event whose areas are not known.
const UNKNOWN: &str = "a pull request event whose paths are not all known, more than 100 or none read, falls in no area, so match.areas does not take it and not.areas refuses it";

/// What doctor says of a route reading paths at delivery.
const DELIVERY: &str = "reads the paths each pull request event's pull request changes from the forge as the event is delivered: one request more for each, through its source's token, API base and the run's request budget, one page of at most 100 paths, a failed read leaving the event in no area";

/// The sources of `config`'s that no view reads kept, of `sources`.
fn unkept<'a>(config: &Config, sources: &'a [SourceName]) -> Vec<&'a str> {
    let keeps = config.store.keeps_pulls();
    sources
        .iter()
        .filter(|source| {
            !config.views.values().any(|view| {
                view.sources.contains(source)
                    && crate::kept::reads_kept((config, keeps), view, source)
            })
        })
        .map(SourceName::as_str)
        .collect()
}

/// Each note on each route of `config` that narrows by areas.
pub(super) fn routed(config: &Config, notes: &mut Vec<String>) {
    for route in &config.routes {
        let route = &route.route;
        if !route.filter.matching.reads_areas() {
            continue;
        }
        let id = route.id.as_str();
        let note = match route.filter.area_paths {
            AreaPaths::Delivery => format!("The route {id} {DELIVERY}; {UNKNOWN}."),
            AreaPaths::Kept => {
                let unkept = unkept(config, &route.filter.sources);
                let kept = if unkept.is_empty() {
                    String::new()
                } else {
                    format!(
                        "; no view reads {} kept, so its events fall in no area: add a kept view of it, or set area_paths = \"delivery\"",
                        unkept.join(", ")
                    )
                };
                format!(
                    "The route {id} takes each pull request event's areas from the paths kept for that pull request where its kept head is the event's, no request more{kept}; {UNKNOWN}."
                )
            }
        };
        notes.push(note);
    }
}
