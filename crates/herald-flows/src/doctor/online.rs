//! What doctor finds by reading secrets, keys and destinations online.

use std::collections::{BTreeMap, BTreeSet};

use herald_config::Config;
use herald_kinds::{Expanded, Polling, Probe, Unlisted, Unpolled};
use herald_ports::{Http, SecretError, Secrets, Signer};

use super::{Findings, Here, gap};
use crate::Connected;
use crate::prepare::signs_as_actor;

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
        if let Err(unkeyed) = signs_as_actor(signer, name, &key).await {
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
    findings.extend(owned(connected).await);
    findings
}

/// What each owner source lists, asked through the `Http` `connected` gives
/// it (ADR-0029): a mistake where a listing passes a cap, or where two
/// members, of one owner or of two, share a project name.
async fn owned<H: Http>(connected: &Connected<H>) -> Findings {
    let mut findings = Findings::default();
    let mut named: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (source, http) in &connected.sources {
        let Polling::Many(expand) = &source.poll else {
            continue;
        };
        let name = source.name.as_str();
        match expand.listed(http, None).await {
            Ok(expanded) => {
                if let Expanded::Listed { missing, .. } = &expanded
                    && !missing.is_empty()
                {
                    findings.mistakes.push(format!(
                        "The source {name}'s member file names {}, which its owner does not hold; correct or remove them.",
                        missing.join(", ")
                    ));
                }
                let members = expanded.members();
                let noun = if members.len() == 1 {
                    "repository"
                } else {
                    "repositories"
                };
                findings
                    .notes
                    .push(format!("The source {name} takes {} {noun}.", members.len()));
                for member in members {
                    match expand.project(&member) {
                        Ok(project) => named
                            .entry(project.to_lowercase())
                            .or_default()
                            .push(format!("{name}'s {}", member.path)),
                        Err(reason) => findings.mistakes.push(format!(
                            "The source {name} names no project for {}: {reason}.",
                            member.path
                        )),
                    }
                }
            }
            Err(Unlisted::Over(reason)) => {
                findings
                    .mistakes
                    .push(format!("The source {name}: {reason}."));
            }
            Err(Unlisted::Failed(Unpolled::Unreachable(reason) | Unpolled::Unreadable(reason))) => {
                findings.gaps.push(gap(
                    &format!("listing the owner of the source \"{name}\""),
                    format!("sources.{name}.owner"),
                    reason,
                ));
            }
        }
    }
    for (project, members) in named.into_iter().filter(|(_, members)| members.len() > 1) {
        findings.mistakes.push(format!(
            "{} are all announced as the project {project}; leave all but one out with exclude, or name a narrower owner.",
            members.join(", ")
        ));
    }
    findings
}
