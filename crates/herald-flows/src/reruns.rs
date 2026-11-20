//! A rule's plans to re-run each flaky check failing on its view's open
//! pull requests: the evidence and the exact call, announced once a run
//! and head to the rule's private routes alone. A rule only plans; a
//! signed-in operator re-runs (ADR-0065).

use herald_config::{Config, Rule, ViewConfig};
use herald_core::{Board, Checks, RerunPlan, RouteId, SourceName, rerun_event};
use herald_kinds::Plan as Planned;
use herald_ports::{Clock, Http, Signer, Store};

use crate::announce::{Ports, Stop, Within, announce_within};
use crate::budget::Budgeted;
use crate::poll::Fresh;
use crate::prepare::Connected;

/// The routes naming the rule `name` that say they are private: the only
/// ones its plans reach.
fn private_routes(config: &Config, name: &str) -> Vec<RouteId> {
    config
        .routes
        .iter()
        .filter(|route| {
            route.private
                && route
                    .route
                    .filter
                    .sources
                    .iter()
                    .any(|source| source.as_str() == name)
        })
        .map(|route| route.route.id.clone())
        .collect()
}

/// The plans of rule `name` for `shown`, the board of `view`: for each
/// flaky check failing on a pull request it shows, at most `rule.most`,
/// the runs on its head that ran it, read through the source that reads
/// its repository, each announced to the rule's private routes.
///
/// # Errors
///
/// Where a source's runs could not be read, the host's limit is near, or
/// a plan makes no event.
pub(crate) async fn planned<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, rule): (&str, &Rule),
    (view, shown): (&ViewConfig, &Board),
    run: &str,
) -> Result<Vec<Fresh>, Stop> {
    let flaky: Vec<(&str, usize)> = shown
        .failures
        .iter()
        .filter(|failure| failure.flaky)
        .map(|failure| (failure.check.as_str(), failure.flips))
        .collect();
    let rows = shown
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .chain(shown.batches.iter().flat_map(|batch| &batch.rows));
    let reading = |repository: &str| {
        connected
            .sources
            .iter()
            .filter(|(prepared, _)| view.sources.contains(&prepared.name))
            .find_map(|(prepared, http)| {
                prepared
                    .queue
                    .as_ref()
                    .filter(|queue| queue.holds(repository))
                    .map(|queue| (queue, http))
            })
    };
    let mut failing: Vec<_> = rows
        .filter_map(|row| match (&row.pull.checks, row.pull.head.as_deref()) {
            (Checks::Failed(names), Some(head)) => Some((&row.pull, head, names)),
            _ => None,
        })
        .flat_map(|(pull, head, names)| {
            flaky
                .iter()
                .filter(|(check, _)| names.iter().any(|name| name == check))
                .map(move |(check, flips)| (pull, head, *check, *flips))
        })
        .filter_map(|(pull, head, check, flips)| {
            reading(&pull.repository).map(|read| (pull, head, check, flips, read))
        })
        .collect();
    failing.truncate(rule.most);
    let routes = private_routes(config, name);
    let source = SourceName::new(name);
    let mut announced = Vec::new();
    for (pull, head, check, flips, (queue, http)) in failing {
        let http = &Budgeted::new(http, ports.budget, ports.clock);
        let plan = queue
            .planned(http, (&pull.repository, head, check), rule.runs)
            .await
            .map_err(|unpolled| http.halted(unpolled))?;
        let runs = match &plan {
            Planned::Runs(runs) => runs.iter().map(Ok).collect(),
            Planned::Unplanned(reason) => vec![Err(*reason)],
        };
        for run_of in runs {
            let planned = RerunPlan {
                pull,
                head,
                check,
                flips,
                run: run_of,
            };
            let event =
                rerun_event(name, planned).map_err(|problem| Stop::Source(problem.to_string()))?;
            let within = Within::routes(&routes);
            let reached =
                announce_within(config, connected, ports, &source, &event, within, run).await;
            announced.push(Fresh {
                event,
                announced: reached,
                held: None,
            });
        }
    }
    Ok(announced)
}
