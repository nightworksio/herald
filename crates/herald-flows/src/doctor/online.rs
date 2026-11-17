//! What doctor finds by reading secrets, keys and destinations online.

use std::collections::{BTreeMap, BTreeSet};

use herald_config::Config;
use herald_core::{Listed, SourceName};
use herald_keys::ed25519_of;
use herald_kinds::{Expand, Expanded, Polling, Probe, Standing, Unlisted, Unpolled};
use herald_ports::{Algorithm, DynHttp, Http, SecretError, Secrets, Signer, Tokens};
use jiff::Timestamp;

use super::{Findings, Gap, Here, gap, unnamed_gap};
use crate::Connected;
use crate::acting::{Claim, Claimant, Claimed};
use crate::installations::{Kept, refreshed_by};
use crate::prepare::PreparedSource;
use crate::prepare::signs_as_actor;

/// Each ActivityPub actor's key and each of the instance's keys in `config`
/// that is set here, read through `secrets` and loaded through `signer`: a
/// mistake where an instance key is no Ed25519 key, or where an actor's
/// cannot sign as the actor.
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
    for (index, reference) in config.federation.keys.iter().enumerate() {
        let Ok(key) = secrets.secret(&reference.secret).await else {
            continue;
        };
        let public = signer.public(Algorithm::Ed25519, &key).await;
        if public.as_deref().ok().and_then(ed25519_of).is_none() {
            findings.mistakes.push(format!(
                "federation.keys[{index}]: the secret {} is not an Ed25519 private key; give one as PKCS#8 PEM, such as openssl genpkey -algorithm ed25519 writes",
                reference.secret
            ));
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
pub async fn online<H: Http, G: Signer>(
    connected: &Connected<H>,
    (signer, now): (&G, Timestamp),
) -> Findings {
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
    findings.extend(owned(connected, (signer, now)).await);
    let sources: Vec<(&PreparedSource, &dyn DynHttp)> = connected
        .sources
        .iter()
        .map(|(source, http)| (source, http as &dyn DynHttp))
        .collect();
    findings.extend(probed(&sources).await);
    findings
}

/// What each of `sources`' own check of what it reaches through its `Http`
/// finds wrong (ADR-0042).
async fn probed(sources: &[(&PreparedSource, &dyn DynHttp)]) -> Findings {
    let mut findings = Findings::default();
    for (source, http) in sources {
        let name = source.name.as_str();
        findings
            .gaps
            .extend(source.poll.probed(*http).await.into_iter().map(|probed| {
                gap(
                    &format!("reading the source \"{name}\""),
                    format!("sources.{name}.{}", probed.key),
                    probed.why,
                )
            }));
    }
    findings
}

/// Whether two of `members` announce as one project and may not share it,
/// as an image and the repository it is linked to may.
pub(super) fn colliding(members: &[(String, Option<Standing>)]) -> bool {
    members.iter().enumerate().any(|(at, (_, mine))| {
        members.iter().skip(at + 1).any(|(_, theirs)| {
            !mine
                .as_ref()
                .zip(theirs.as_ref())
                .is_some_and(|(mine, theirs)| mine.shares(theirs))
        })
    })
}

/// That each route taking the owner source `source`, whose member `member`
/// announces as `project`, announces nothing of it, where the route
/// announces each project as an actor of its own and the project names
/// none; each actor it does name held in `claimed` (ADR-0036).
fn unnamed<H>(
    connected: &Connected<H>,
    (source, expand, member): (&SourceName, &dyn Expand, &Listed),
    project: &str,
    claimed: &mut Claimed,
) -> Vec<Gap> {
    let mut gaps = Vec::new();
    for (index, (route, _)) in connected.routes.iter().enumerate() {
        let taken = route.projects.as_ref().is_some_and(|projects| {
            projects
                .owner(source)
                .is_some_and(|sourced| projects.takes((expand, sourced), member, project))
        });
        let Some((actor, named)) = route
            .reached
            .federates()
            .filter(|_| taken)
            .and_then(|actor| Some((actor, actor.names(project)?)))
        else {
            continue;
        };
        let label = format!("{}'s {} on routes[{index}]", source.as_str(), member.path);
        match named {
            Ok(name) => claimed.hold(
                (actor.domain(), &name),
                Claim {
                    claimant: Claimant::Member(source.clone(), member.id.clone()),
                    project: project.to_owned(),
                },
                (label, true),
            ),
            Err(why) => gaps.push(unnamed_gap(
                &route.config,
                index,
                project,
                &format!("{}'s {}: {why}", source.as_str(), member.path),
            )),
        }
    }
    gaps
}

/// What the members of the owner source `name` lack of the webhooks its
/// `member_hooks` registers at `url`, asked through `http`: each member
/// without one, and, where each round registers them, why its token may
/// not.
async fn hooks<H: Http>(
    (name, expand, http): (&str, &dyn Expand, &H),
    (url, rounds): (&str, bool),
    members: &[Listed],
) -> Findings {
    let mut findings = Findings::default();
    if rounds && let Err(reason) = expand.may_register(http).await {
        findings.mistakes.push(format!(
            "The source {name}'s token cannot register its members' webhooks: {reason}; give it the api scope and Maintainer access, or set member_hooks = \"setup\"."
        ));
    }
    let mut unhooked = Vec::new();
    for member in members {
        match expand.hooked(http, member, url).await {
            Ok(true) => {}
            Ok(false) => unhooked.push(member.path.as_str()),
            Err(Unpolled::Unreachable(reason) | Unpolled::Unreadable(reason)) => {
                findings.gaps.push(gap(
                    &format!("{}'s webhooks", member.path),
                    format!("sources.{name}.member_hooks"),
                    reason,
                ));
            }
        }
    }
    if !unhooked.is_empty() {
        findings.mistakes.push(format!(
            "The source {name}'s members {} have no webhook to herald; run herald hooks {name}.",
            unhooked.join(", ")
        ));
    }
    findings
}

/// The sources whose App's token could not be minted at `now`, signed by
/// `signer`, each named as a gap into `findings`.
async fn unminted<H: Http, G: Signer>(
    connected: &Connected<H>,
    (signer, now): (&G, Timestamp),
    findings: &mut Findings,
) -> BTreeMap<SourceName, herald_wire::github_app::Unminted> {
    let unminted = refreshed_by(
        &connected.mintings,
        (signer, None::<Kept<'_, herald_ports::Unkept>>),
        now,
    )
    .await
    .unminted;
    for (name, stop) in &unminted {
        findings.gaps.push(gap(
            &format!(
                "minting the token the source \"{}\" reads with",
                name.as_str()
            ),
            format!("sources.{}.app", name.as_str()),
            stop.to_string(),
        ));
    }
    unminted
}

/// Each GitHub App that keeps its installation tokens, a token minted
/// through `connected` at `now`, signed by `signer`, and kept in the store
/// `kind`'s `tokens`: a gap for each App whose token the store does not
/// keep (ADR-0041).
pub async fn kept_tokens<H: Http, G: Signer, T: Tokens>(
    connected: &Connected<H>,
    (signer, tokens, now): (&G, Option<&T>, Timestamp),
    kind: &str,
) -> Findings {
    let mut findings = Findings::default();
    let refreshed = refreshed_by(
        &connected.mintings,
        (signer, tokens.map(|tokens| (tokens, kind))),
        now,
    )
    .await;
    let mut named = BTreeSet::new();
    for unkept in refreshed.unkept {
        if named.insert(unkept.app.clone()) {
            findings.gaps.push(gap(
                &format!(
                    "keeping the GitHub App {}'s installation tokens",
                    unkept.app
                ),
                format!("github_apps.{}.store_token", unkept.app),
                unkept.why,
            ));
        }
    }
    findings
}

/// What each owner source lists, asked through the `Http` `connected` gives
/// it (ADR-0029): a mistake where a listing passes a cap, or where two
/// members, of one owner or of two, share a project name.
async fn owned<H: Http, G: Signer>(
    connected: &Connected<H>,
    (signer, now): (&G, Timestamp),
) -> Findings {
    let mut findings = Findings::default();
    let unminted = unminted(connected, (signer, now), &mut findings).await;
    let mut named: BTreeMap<String, Vec<(String, Option<Standing>)>> = BTreeMap::new();
    let mut claimed = Claimed::of_connected(connected);
    for (source, http) in connected
        .sources
        .iter()
        .filter(|(source, _)| !unminted.contains_key(&source.name))
    {
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
                if let Some((url, secret)) = &source.hook {
                    findings.extend(
                        hooks(
                            (name, expand.as_ref(), http),
                            (url, secret.is_some()),
                            &members,
                        )
                        .await,
                    );
                }
                let (one, several) = expand.member_nouns();
                let noun = if members.len() == 1 { one } else { several };
                findings
                    .notes
                    .push(format!("The source {name} takes {} {noun}.", members.len()));
                for member in members {
                    match expand.project(&member) {
                        Ok(project) => {
                            findings.gaps.extend(unnamed(
                                connected,
                                (&source.name, expand.as_ref(), &member),
                                &project,
                                &mut claimed,
                            ));
                            named.entry(project.to_lowercase()).or_default().push((
                                format!("{name}'s {}", member.path),
                                expand.standing(&member),
                            ));
                        }
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
                let key = expand.owner_key();
                findings.gaps.push(gap(
                    &format!("listing the {key} of the source \"{name}\""),
                    format!("sources.{name}.{key}"),
                    reason,
                ));
            }
        }
    }
    for (project, members) in named.into_iter().filter(|(_, members)| colliding(members)) {
        let labels: Vec<String> = members.into_iter().map(|(label, _)| label).collect();
        findings.mistakes.push(format!(
            "{} are all announced as the project {project}; leave all but one out with exclude, or name a narrower owner.",
            labels.join(", ")
        ));
    }
    findings.mistakes.extend(claimed.collisions(true));
    findings
}
