//! What a Mastodon server says of an account's token and of how much a
//! route's statuses may hold, asked with nothing posted (ADR-0002,
//! ADR-0004).

use herald_kinds::Probe;
use herald_ports::Http;
use serde::Deserialize;

use crate::mastodon::{LIMIT, Mastodon};
use crate::options::RouteOptions;
use crate::request::get;

/// The platform, as a probe names it.
const MASTODON: &str = "Mastodon";

/// What a status may hold, as `/api/v2/instance` says.
#[derive(Debug, Deserialize)]
struct Instance {
    configuration: Configuration,
}

#[derive(Debug, Deserialize)]
struct Configuration {
    statuses: Statuses,
}

#[derive(Debug, Deserialize)]
struct Statuses {
    max_characters: usize,
    characters_reserved_per_url: usize,
}

/// What the server's own `statuses` say of `route`'s `limit` and
/// `link_length`.
fn measured(route: &RouteOptions, statuses: &Statuses) -> Vec<Probe> {
    let mut probes = Vec::new();
    let limit = route.limit.unwrap_or(LIMIT);
    let most = statuses.max_characters;
    if limit > most {
        probes.push(Probe::Route {
            key: Some("limit"),
            why: format!("the server takes at most {most} characters in a status, fewer than the {limit} herald writes; set limit = {most}"),
        });
    } else if limit < most {
        probes.push(Probe::Said(format!("the server takes {most} characters in a status, more than the {limit} herald writes; set limit = {most} to use them")));
    }
    let link = route.link_length();
    let reserved = statuses.characters_reserved_per_url;
    if reserved > link {
        probes.push(Probe::Route {
            key: Some("link_length"),
            why: format!("the server counts a link as {reserved} characters, more than the {link} herald counts; set link_length = {reserved}"),
        });
    } else if reserved < link {
        probes.push(Probe::Said(format!("the server counts a link as {reserved} characters, fewer than the {link} herald counts; set link_length = {reserved}")));
    }
    probes
}

/// What the server `mastodon` reaches says, asked through `http`, of its
/// token and of the route's limits.
pub(crate) async fn probe<H: Http>(mastodon: &Mastodon, http: &H) -> Vec<Probe> {
    let verified = http
        .send(get(mastodon, "/api/v1/accounts/verify_credentials"))
        .await;
    let taken = Probe::credential(MASTODON, &verified);
    if taken != Probe::Taken {
        return vec![taken];
    }
    let instance = http.send(get(mastodon, "/api/v2/instance")).await;
    let read = instance
        .ok()
        .filter(herald_ports::Response::succeeded)
        .and_then(|answer| serde_json::from_slice::<Instance>(&answer.body).ok());
    let mut probes = vec![taken];
    match read {
        Some(instance) => probes.extend(measured(
            &mastodon.route,
            &instance.configuration.statuses,
        )),
        None => probes.push(Probe::Said("the server does not say how many characters a status holds at /api/v2/instance, so limit and link_length are not checked against it".to_owned())),
    }
    probes
}
