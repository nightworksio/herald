//! Each rule whose window began and was not yet watched: its view's open
//! pull requests read, and a nudge announced for each that waited past one
//! of its stages, once a stage and spell of waiting (ADR-0056).

use herald_config::{Config, Rule};
use herald_core::{SourceName, grouped, nudge_event, nudges};
use herald_ports::{Clock, Http, Signer, Store};

use crate::announce::{Ports, Stop, announce};
use crate::deciding::decided_rows;
use crate::poll::{Fresh, Sweep};
use crate::prepare::Connected;
use crate::report::{Reads, due, settled, view_pulls};

/// The rule `name`, where its window began and was not yet watched: a
/// nudge of each row of its view that reached a stage, the longest
/// waiting first, up to its `most`, each seen as its repository is.
pub(crate) async fn nudged<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, rule): (&str, &Rule),
    run: &str,
    reads: &mut Reads,
) -> Option<Sweep> {
    let (config, connected, ports) = flow;
    let start = rule.watch.schedule.window(ports.clock.now())?;
    let (identity, record) = match due(ports, name, start).await {
        Ok(due) => due?,
        Err(stop) => return Some(Sweep::Stopped(stop)),
    };
    let view = config.views.get(&rule.watch.view)?;
    let pulls = match view_pulls(flow, (&rule.watch.view, view), reads).await {
        Ok(pulls) => pulls,
        Err(stop) => return Some(Sweep::Stopped(stop)),
    };
    let shown = grouped(&view.view, decided_rows(flow, view, &pulls).await);
    let source = SourceName::new(name);
    let mut announced = Vec::new();
    for nudge in nudges(&shown, &rule.stages, rule.most) {
        let event = match nudge_event(name, nudge) {
            Ok(event) => event,
            Err(problem) => return Some(Sweep::Stopped(Stop::Source(problem.to_string()))),
        };
        let reached = announce(config, connected, ports, &source, &event, run).await;
        announced.push(Fresh {
            event,
            announced: reached,
            held: None,
        });
    }
    let cursor = settled(ports, (name, identity), record.as_ref(), &announced).await;
    Some(Sweep::Reported {
        pulls: pulls.len(),
        announced,
        cursor,
    })
}
