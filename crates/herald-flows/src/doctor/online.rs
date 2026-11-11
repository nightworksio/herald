use std::collections::BTreeSet;

use herald_config::Config;
use herald_kinds::Probe;
use herald_ports::{Http, SecretError, Secrets, Signer};

use super::{Findings, Here, gap};
use crate::Connected;
use crate::prepare::keyed;

/// Each ActivityPub actor's key in `config` that is set here, read through
/// `secrets` and loaded through `signer`: a mistake where it cannot sign as
/// the actor.
pub async fn keys<S: Secrets, G: Signer>(config: &Config, secrets: &S, signer: &G) -> Findings {
    let mut findings = Findings::default();
    for (name, destination) in &config.destinations {
        let Some(reference) = destination.credential().filter(|_| destination.acts()) else {
            continue;
        };
        let Ok(key) = secrets.secret(&reference.secret).await else {
            continue;
        };
        if let Err(unkeyed) = keyed(signer, name, &key).await {
            findings.mistakes.push(unkeyed.to_string());
        }
    }
    findings
}

/// Every secret `config` names, read through `secrets`: one that is not set
/// here is noted, or a gap where `here` holds the config to what is set
/// here, and one that cannot be read is a gap.
pub async fn secrets<S: Secrets>(config: &Config, secrets: &S, here: &Here) -> Findings {
    let mut findings = Findings::default();
    let mut unset = Vec::new();
    for name in config.secrets() {
        let why = match secrets.secret(name).await {
            Ok(_) => continue,
            Err(SecretError::Missing { .. }) if !here.online => {
                unset.push(name);
                continue;
            }
            Err(error) => error.to_string(),
        };
        let keys = config.keys_naming(name);
        let key = if keys.is_empty() {
            name.to_owned()
        } else {
            keys.join(", ")
        };
        findings
            .gaps
            .push(gap(&format!("reading the secret {name}"), key, why));
    }
    if !unset.is_empty() {
        findings.notes.push(format!(
            "Nothing that reads {} is checked, as {} not set here; herald doctor --online reads every secret.",
            unset.join(", "),
            if unset.len() == 1 { "it is" } else { "they are" }
        ));
    }
    findings
}

/// What each route's platform says of it, asked through the `Http`
/// `connected` gives the route, with nothing posted (ADR-0002).
pub async fn online<H: Http>(connected: &Connected<H>) -> Findings {
    let mut findings = Findings::default();
    let mut refused = BTreeSet::new();
    let mut taken = BTreeSet::new();
    for (index, (route, http)) in connected.routes.iter().enumerate() {
        let destination = route.config.route.destination.as_str();
        let id = route.config.route.id.as_str();
        let delivery = &route.config.route.delivery;
        for probe in route.reached.probe(http, delivery).await {
            match probe {
                Probe::Taken => {
                    taken.insert(destination);
                }
                Probe::Credential(why) => {
                    if refused.insert(destination) {
                        findings.gaps.push(gap(
                            &format!("posting through the destination \"{destination}\""),
                            format!("destinations.{destination}"),
                            why,
                        ));
                    }
                }
                Probe::Route { key, why } => findings.gaps.push(gap(
                    &format!("announcing on the route {id}"),
                    key.map_or_else(
                        || format!("routes[{index}]"),
                        |key| format!("routes[{index}].{key}"),
                    ),
                    why,
                )),
                Probe::Said(said) => findings.notes.push(format!("The route {id}: {said}")),
            }
        }
    }
    for destination in taken.difference(&refused) {
        findings.notes.push(format!(
            "The destination \"{destination}\" takes its credential."
        ));
    }
    findings
}
