//! Forge webhooks: each verified with its source's secret and admitted once
//! before it is read, and the release it publishes left to announce, or the
//! source whose tag it made left to poll (ADR-0007, ADR-0013, ADR-0032).

use std::collections::BTreeMap;

use herald_core::{Scheme, SourceName, status};
use herald_kinds::{Hooked, Source};
use herald_ports::{Clock, Secret, Store, Verifier};

use super::jobs::{Job, Member};
use super::replies::{Answered, missing, rejection, text_reply, unread};
use super::{Inbound, Serving};
use crate::verification::{Checking, Verified, admitted, verified};

/// The secret each forge source's webhooks are signed with, by the source,
/// and each GitHub App's, by the App (ADR-0041).
#[derive(Debug, Clone, Default)]
pub struct Webhooks {
    pub(crate) sources: BTreeMap<SourceName, Secret>,
    pub(crate) apps: BTreeMap<String, Secret>,
}

/// What a delivery whose release is left to announce is answered.
const ANNOUNCES: &str = "herald announces the release";
/// What a delivery whose tag is left to a poll is answered.
const POLLS: &str = "herald polls the source for the tag";
/// What a delivery saying the owner's repositories changed is answered.
const RELISTS: &str = "herald lists the owner's repositories again";
/// What an App's delivery left to several sources is answered.
const FANS: &str = "herald takes the delivery for each source the App reaches";
/// What an App's delivery no source takes is answered.
const UNTAKEN: &str = "no source the App reaches takes the delivery";
/// What a delivery whose change kept mode folds is answered (ADR-0061).
const FOLDS: &str = "herald keeps the pull request's change";

/// The answer to a delivery whose work is left to `job`, saying `what`
/// herald does.
fn accepted(job: Job, what: &str) -> Answered {
    Answered {
        reply: text_reply(status::ACCEPTED, what).reply,
        refused: None,
        job: Some(job),
    }
}

/// The delivery `inbound`, signed under `scheme` with `secret`, verified
/// and admitted once, or the answer refusing it.
async fn checked<S: Store, C: Clock, G, V: Verifier, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    (scheme, secret): (Scheme, &Secret),
    inbound: &Inbound<'_>,
) -> Result<Verified, Box<Answered>> {
    let store = serving.ports.store;
    let now = serving.ports.clock.now();
    let verified = verified(
        serving.verifier,
        scheme,
        secret.expose(),
        inbound.headers,
        inbound.body,
        Checking {
            now,
            bounds: &serving.config.server.inbound,
        },
    )
    .await
    .map_err(|rejected| Box::new(rejection(&rejected)))?;
    admitted(store, serving.admission, scheme.name, &verified, now)
        .await
        .map_err(|rejected| Box::new(rejection(&rejected)))?;
    Ok(verified)
}

/// The work `hooked` leaves for the source `source`, of a delivery
/// verified as `verified` under `scheme`, and what herald says it does;
/// none where it leaves nothing, and why.
fn job_of(
    source: SourceName,
    hooked: Hooked,
    (scheme, verified): (Scheme, Verified),
) -> Result<(Job, &'static str), &'static str> {
    let polled = |relist| Job::Poll {
        source: source.clone(),
        scheme,
        verified: verified.clone(),
        relist,
    };
    match hooked {
        Hooked::Event(event) => Ok((
            Job::Announce {
                source,
                event,
                member: None,
                scheme,
                verified,
            },
            ANNOUNCES,
        )),
        Hooked::Member {
            event,
            id,
            visibility,
        } => Ok((
            Job::Announce {
                source,
                event,
                member: Some(Member { id, visibility }),
                scheme,
                verified,
            },
            ANNOUNCES,
        )),
        Hooked::Poll => Ok((polled(false), POLLS)),
        Hooked::Relist => Ok((polled(true), RELISTS)),
        Hooked::Nothing(why) => Err(why),
    }
}

/// The fold of the change a delivery with `inbound`'s headers and body
/// makes to the pull requests of `source`, read by `hooking`, where a view
/// reads the source kept (ADR-0061).
///
/// # Errors
///
/// Why the delivery could not be read.
fn fold_of<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    (source, hooking): (&SourceName, &Source),
    inbound: &Inbound<'_>,
    (scheme, verified): (Scheme, &Verified),
) -> Result<Option<Job>, String> {
    if !crate::kept::folds((serving.config, serving.ports.store), source) {
        return Ok(None);
    }
    Ok(hooking
        .folded(inbound.headers, inbound.body)?
        .map(|folding| Job::Fold {
            source: source.clone(),
            folding,
            scheme,
            verified: verified.clone(),
        }))
}

/// `found`, the work a delivery leaves, with `fold` where there is one,
/// and what herald says it does; why it does nothing where it leaves no
/// work at all.
fn with_fold(
    found: Result<(Job, &'static str), &'static str>,
    fold: Option<Job>,
    (scheme, verified): (Scheme, &Verified),
) -> Result<(Job, &'static str), &'static str> {
    match (found, fold) {
        (found, None) => found,
        (Err(_), Some(fold)) => Ok((fold, FOLDS)),
        (Ok((job, what)), Some(fold)) => Ok((
            Job::Fanned {
                jobs: vec![job, fold],
                scheme,
                verified: verified.clone(),
            },
            what,
        )),
    }
}

/// A webhook of the source `name`, verified and admitted, and the release
/// it publishes left to announce.
pub(super) async fn hooked<S: Store, C: Clock, G, V: Verifier, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    name: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let source = SourceName::new(name);
    let Some((hooking, scheme, secret)) = serving
        .config
        .sources
        .get(&source)
        .zip(serving.webhooks.sources.get(&source))
        .and_then(|(hooking, secret)| hooking.scheme().map(|scheme| (hooking, scheme, secret)))
    else {
        return missing("webhook of such a source");
    };
    let verified = match checked(serving, (scheme, secret), inbound).await {
        Ok(verified) => verified,
        Err(refused) => return *refused,
    };
    let fold = fold_of(serving, (&source, hooking), inbound, (scheme, &verified));
    match (hooking.hooked(inbound.headers, inbound.body), fold) {
        (Ok(hooked), Ok(fold)) => {
            let found = job_of(source, hooked, (scheme, verified.clone()));
            match with_fold(found, fold, (scheme, &verified)) {
                Ok((job, what)) => accepted(job, what),
                Err(why) => text_reply(status::OK, why),
            }
        }
        (Err(error), _) | (_, Err(error)) => {
            unread(serving.ports.store, scheme.name, &verified, error).await
        }
    }
}

/// A webhook of the GitHub App `name`, verified and admitted, and left to
/// each source reading as the App that takes it: an owner source whose
/// owner holds its repository, or the source of that repository
/// (ADR-0041).
pub(super) async fn app_hooked<S: Store, C: Clock, G, V: Verifier, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    name: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let reading: Vec<(&SourceName, &herald_kinds::Source)> = serving
        .config
        .sources
        .iter()
        .filter(|(_, source)| {
            source
                .installed()
                .is_some_and(|installed| installed.app == name)
        })
        .collect();
    let Some((scheme, secret)) = serving
        .webhooks
        .apps
        .get(name)
        .zip(reading.iter().find_map(|(_, source)| source.scheme()))
        .map(|(secret, scheme)| (scheme, secret))
    else {
        return missing("webhook of such an App");
    };
    let verified = match checked(serving, (scheme, secret), inbound).await {
        Ok(verified) => verified,
        Err(refused) => return *refused,
    };
    let mut jobs: Vec<(Job, &str)> = reading
        .into_iter()
        .filter_map(|(source, hooking)| {
            let hooked = hooking.hooked(inbound.headers, inbound.body).ok()?;
            let fold = fold_of(serving, (source, hooking), inbound, (scheme, &verified))
                .unwrap_or_default();
            let found = job_of(source.clone(), hooked, (scheme, verified.clone()));
            with_fold(found, fold, (scheme, &verified)).ok()
        })
        .collect();
    match jobs.len() {
        0 => text_reply(status::OK, UNTAKEN),
        1 => {
            let (job, what) = jobs.remove(0);
            accepted(job, what)
        }
        _ => accepted(
            Job::Fanned {
                jobs: jobs.into_iter().map(|(job, _)| job).collect(),
                scheme,
                verified,
            },
            FANS,
        ),
    }
}
