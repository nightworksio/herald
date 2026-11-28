//! A test message posted to each route's destination, as `herald doctor
//! --post` asks.

use std::collections::BTreeMap;

use herald_config::{Config, RouteConfig};
use herald_core::{Delivery, Draft, Event, Kind, Problem, TemplateName, Templates, Visibility};
use herald_ports::{Clock, Http, Signer, Store};
use jiff::Timestamp;

use crate::announce::{Announced, Outcome, Ports, Run, Stop};
use crate::prepare::Connected;
use crate::preview::{Previewed, rendered};

/// The template a test message is rendered with: its own title and text.
const TESTED: &str = "herald-test";
/// The project a test message names.
const PROJECT: &str = "herald";

/// The test message `route` of `config` posts in `run` at `now`, worded by
/// its destination's wording, and the route as it posts it: through
/// [`TESTED`] alone, with no crosspost, thread, pin or mention.
fn tested(
    config: &Config,
    route: &RouteConfig,
    (run, now): (&str, Timestamp),
) -> (Result<Event, Problem>, RouteConfig) {
    let id = route.route.id.as_str();
    let wording = config
        .destinations
        .get(&route.route.destination)
        .and_then(|destination| destination.answering())
        .map(|answering| answering.wording)
        .unwrap_or_default();
    let (title, body) = wording.test(id);
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
    let mut posting = route.clone();
    posting.route.template = Some(TemplateName::new(TESTED));
    posting.route.delivery = Delivery::default();
    (drafted, posting)
}

/// The templates a test message renders with.
fn templates(config: &Config) -> Templates {
    Templates::verbatim(&TemplateName::new(TESTED), config.render)
}

/// The test message [`trial`] posts on `route` of `config` in `run` at
/// `now`, rendered as its destination would show it, as `herald preview`
/// renders an event: no secret is resolved and nothing is sent.
#[must_use]
pub fn trial_previewed(config: &Config, route: &RouteConfig, at: (&str, Timestamp)) -> Previewed {
    let (drafted, posting) = tested(config, route, at);
    match drafted {
        Ok(event) => rendered(config, &posting, &templates(config), (&event).into()),
        Err(problem) => Previewed {
            route: posting.route.id,
            destination: posting.route.destination,
            against: None,
            rendered: Err(problem.to_string()),
        },
    }
}

/// A test message posted to each route's destination through the
/// announcement's own steps, as `herald doctor --post` asks: a plain post,
/// with no crosspost, thread, pin or mention, worded by the destination's
/// wording and kept in `ports`' store, which `run` holds (ADR-0002). A route
/// to one of herald's own actors or feeds posts nowhere, and is left out.
pub async fn trial<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    run: &str,
) -> Vec<Announced> {
    let templates = templates(config);
    let now = ports.clock.now();
    let mut tried = Vec::new();
    for (route, http) in &connected.routes {
        if route.reached.follows().is_some()
            || route.reached.feeds().is_some()
            || route.reached.pushes().is_some()
        {
            continue;
        }
        let (drafted, posting) = tested(config, &route.config, (run, now));
        let outcome = match drafted {
            Ok(event) => {
                let mut tested = route.clone();
                tested.config = posting;
                Run {
                    ports,
                    route: &tested,
                    kinds: &config.kinds,
                    http,
                    holder: run,
                    lease: config.announce.lease,
                    seen: Visibility::Public,
                    view: None,
                    advised: false,
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
        tried.push(Announced {
            route: route.config.route.id.clone(),
            outcome,
        });
    }
    tried
}
