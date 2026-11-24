//! A rule's alerts on its view's release, one audience of its routes at a
//! time, each judged as that audience's board tells it (ADR-0085).

use std::collections::{BTreeMap, HashSet};

use herald_config::{Config, Rule, ViewConfig};
use herald_core::{
    Alerting, Cursor, ReleaseVerdict, Row, SourceName, Told, Visibility, readied, readied_event,
    told_of,
};
use herald_ports::{Clock, Http, Signer, Store, Versioned};
use jiff::tz::TimeZone;

use crate::announce::{Ports, Stop, Within, announce_within};
use crate::poll::Fresh;
use crate::prepare::Connected;
use crate::report::{Read, audiences};

/// What a rule's alerts on its view's release did: each announced, and the
/// verdict each audience was told, by who it may see.
pub(crate) type Alerted = (Vec<Fresh>, BTreeMap<Visibility, ReleaseVerdict>);

/// The alerts of the rule `name` on the release of `view`, read as `read`
/// with its rows decided as `decided`: for each audience of the routes
/// naming the rule, every row to the private routes and the public
/// repositories' rows to the rest, the verdict judged beside the rows
/// withheld from it and the base branches it may see, as its board would
/// tell it; each alert announced to that audience's routes alone, seen as
/// it is, the verdict each was told before read from `record`.
///
/// # Errors
///
/// Where an alert's event could not be made.
pub(crate) async fn alerted<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, rule): (&str, &Rule),
    (view, read, decided): (&ViewConfig, &Read, &[Row]),
    (record, run): (Option<&Versioned<Cursor>>, &str),
) -> Result<Alerted, Stop> {
    let mut alerted: Alerted = (Vec::new(), BTreeMap::new());
    if rule.release == Alerting::default() {
        return Ok(alerted);
    }
    let strictest = read.strictest();
    let private = crate::poll::bounded(config, strictest);
    let told = record.map(|record| &record.value.verdicts);
    let since = record
        .and_then(|record| record.value.seen.first())
        .map_or("", |identity| identity.as_str());
    let now = ports.clock.now();
    let today = now.to_zoned(TimeZone::UTC).date();
    let source = SourceName::new(name);
    let judged = audiences((config, name), decided, (strictest, private), &[])
        .into_iter()
        .filter_map(|(rows, audience, routes, _)| {
            let shown: HashSet<&str> = rows.iter().map(|row| row.pull.url.as_str()).collect();
            let withheld: Vec<Row> = decided
                .iter()
                .filter(|row| !shown.contains(row.pull.url.as_str()))
                .cloned()
                .collect();
            let reads = (
                read.overviews.bases.as_slice(),
                read.overviews.header.as_ref(),
            );
            let judging = told_of(&view.view, (rows, &withheld), reads, audience)?;
            Some((judging, audience, routes))
        });
    for ((readiness, header), audience, routes) in judged.collect::<Vec<_>>() {
        let telling = Told {
            audience,
            readiness: &readiness,
            header: header.as_ref(),
        };
        let last = told.and_then(|told| told.get(&audience)).copied();
        alerted.1.insert(audience, ReleaseVerdict::of(&readiness));
        for alert in readied(rule.release, telling, last, today) {
            let event = readied_event((name, &rule.watch.view), telling, alert, (since, now))
                .map_err(|problem| Stop::Source(problem.to_string()))?;
            let within = Within::routes(&routes);
            let announced =
                announce_within(config, connected, ports, &source, &event, within, run).await;
            alerted.0.push(Fresh {
                event,
                announced,
                held: None,
            });
        }
    }
    Ok(alerted)
}
