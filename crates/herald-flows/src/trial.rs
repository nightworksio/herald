use std::collections::BTreeMap;

use herald_config::Config;
use herald_core::{Delivery, Draft, Event, Kind, TemplateName, Templates};
use herald_ports::{Clock, Http, Signer, Store};

use crate::announce::{Announced, Outcome, Ports, Run, Stop};
use crate::prepare::Connected;

/// The template a test message is rendered with: its own title and text.
const TESTED: &str = "herald-test";
/// The project a test message names.
const PROJECT: &str = "herald";

/// A test message posted to each route's destination through the
/// announcement's own steps, as `herald doctor --post` asks: a plain post,
/// with no crosspost, thread, pin or mention, worded by the destination's
/// wording and kept in `ports`' store, which `run` holds (ADR-0002). A route
/// to one of herald's own actors posts nowhere, and is left out.
pub async fn trial<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    run: &str,
) -> Vec<Announced> {
    let template = TemplateName::new(TESTED);
    let templates = Templates::verbatim(&template, config.render);
    let now = ports.clock.now();
    let mut tried = Vec::new();
    for (route, http) in &connected.routes {
        if route.reached.follows().is_some() {
            continue;
        }
        let id = route.config.route.id.clone();
        let wording = config
            .destinations
            .get(&route.config.route.destination)
            .and_then(|destination| destination.answering())
            .map(|answering| answering.wording)
            .unwrap_or_default();
        let (title, body) = wording.test(id.as_str());
        let drafted = Event::normalise(Draft {
            identity: format!("{PROJECT}/test/{run}/{now}"),
            kind: Kind::MESSAGE,
            project: PROJECT.to_owned(),
            title,
            body,
            links: BTreeMap::new(),
            attributes: BTreeMap::new(),
            occurred: now,
        });
        let outcome = match drafted {
            Ok(event) => {
                let mut tested = route.clone();
                tested.config.route.template = template.clone();
                tested.config.route.delivery = Delivery::default();
                Run {
                    ports,
                    route: &tested,
                    kinds: &config.kinds,
                    http,
                    holder: run,
                    lease: config.announce.lease,
                }
                .announce(&event, &templates)
                .await
            }
            Err(problem) => Outcome::Stopped {
                landed: Vec::new(),
                remaining: Vec::new(),
                reason: Stop::Template(problem.to_string()),
            },
        };
        tried.push(Announced { route: id, outcome });
    }
    tried
}
