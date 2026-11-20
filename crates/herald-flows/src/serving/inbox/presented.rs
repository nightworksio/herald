//! A push presenting a CI run's ID token in place of a peer's signature
//! (ADR-0064): no larger than a peer may send, its token taken by an app,
//! standing for the one name of the app's the source grants, within that
//! name's rate; then its envelope proved by this herald's newest key, as
//! made by itself in the name's stead when it is taken where the token is
//! taken once, else when the token was issued, and taken as a signed push
//! is.

use herald_config::{Peer, Replay, Taking};
use herald_core::{Audited, SourceName};
use herald_kinds::{PublicKey, Trust};
use herald_peers::digest::content_digest;
use herald_peers::discovery::Instance;
use herald_peers::feed::{Envelope, identity, made_at};
use herald_peers::keys::method;
use herald_peers::proof::unproved;
use herald_ports::{Algorithm, DynVerifier, Http, Signer, Store, Verifier};
use jiff::Timestamp;

use super::super::discovery::publics;
use super::super::{Inbound, Serving};
use super::{Counted, PAST, PAST_RATE, Run, Sender, Verdict, counted, grant, granted};
use crate::oidc::taken;
use crate::peering::bounds;
use crate::verification::Admission;

/// Why a push is not proved where this herald holds no key that proves it.
const NO_KEY: &str = "herald holds no [federation] key that proves what it takes";
/// What follows `oidc ` in the key id a push presenting an ID token is
/// kept under: the token's issuer.
const ISSUED_BY: &str = "oidc ";

/// Who sent the push `inbound`, presenting the ID token `token`, into the
/// source `name` served `here`, and what it is answered.
pub(super) async fn judged<S: Store, C, G: Signer, V: Verifier, H: Http, P: Http, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    (inbound, here): (&Inbound<'_>, &Instance<'_>),
    (name, token): (&str, &str),
    now: Timestamp,
) -> (Sender, Verdict) {
    if inbound.body.len() > Peer::MOST {
        return (
            Sender::Unknown,
            Verdict::Large(format!("more than the {} bytes a run may send", Peer::MOST)),
        );
    }
    let config = serving.config;
    let verifier: &dyn DynVerifier = serving.verifier;
    let again = serving.admission == Admission::Again;
    let held = match taken(
        (config, serving.ports.store, verifier),
        &serving.connected.issuers,
        (token, Taking::Push, again),
        now,
    )
    .await
    {
        Ok(held) => held,
        Err(why) if why.unavailable() => {
            return (Sender::Unknown, Verdict::Unavailable(why.to_string()));
        }
        Err(why) => return (Sender::Unknown, Verdict::Unsigned(why.to_string())),
    };
    let granting = config
        .sources
        .get(&SourceName::new(name))
        .and_then(|source| source.grants());
    let Some(peer) = held
        .stands_for
        .iter()
        .find(|peer| granting.is_some_and(|grants| grants.contains_key(*peer)))
    else {
        return (
            Sender::Uncounted(held.app.to_owned()),
            Verdict::Forbidden("the source grants no name the app stands for".to_owned()),
        );
    };
    let envelope = made_at(inbound.body, &here.discovery());
    let run = Run {
        peer: peer.clone(),
        keyid: Audited::named(&format!("{ISSUED_BY}{}", held.entry.issuer)),
        evidence: held
            .evidence
            .iter()
            .map(|(claim, value)| (claim.clone(), Audited::named(value)))
            .collect(),
        identity: envelope
            .as_ref()
            .ok()
            .map(|envelope| Audited::named(identity(envelope)))
            .filter(|identity| !identity.is_empty()),
        digest: content_digest(inbound.body),
    };
    match counted(serving, (peer, Peer::RATE), now).await {
        Ok(Counted::Within) => {}
        Ok(Counted::FirstPast) => return (Sender::Run(run), Verdict::Limited(PAST_RATE)),
        Ok(Counted::Past) => return (Sender::Uncounted(peer.clone()), Verdict::Limited(PAST)),
        Err(verdict) => return (Sender::Uncounted(peer.clone()), verdict),
    }
    let verdict = match grant(config, (peer, name), now) {
        Ok(granting) => {
            let (issued, expires) = held.valid;
            let created = match held.replay {
                Replay::Once => now.clamp(issued, expires.max(issued)),
                Replay::UntilExpiry => issued,
            };
            match proved(serving, here, envelope, created).await {
                Ok((body, public)) => {
                    let trust = trusted(serving, here, public, now);
                    granted(serving, (peer, name), granting, (&body, &trust), now).await
                }
                Err(refused) => refused,
            }
        }
        Err(refused) => refused,
    };
    (Sender::Run(run), verdict)
}

/// The envelope `made` as made by this herald, served `here`: its path
/// this herald alone, proved by its newest key, the first of
/// `[federation] keys`, as of `created`, with the public half of that key.
///
/// # Errors
///
/// The body is no envelope, or this herald holds no key that proves it.
async fn proved<S, C, G: Signer, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    here: &Instance<'_>,
    made: Result<Envelope, String>,
    created: Timestamp,
) -> Result<(Vec<u8>, PublicKey), Verdict> {
    let envelope = made.map_err(unread)?;
    let key = serving.connected.instance.first().ok_or(unproven(NO_KEY))?;
    let public = publics(serving.ports.signer, std::slice::from_ref(key))
        .await
        .map_err(unproven)?
        .first()
        .copied()
        .ok_or(unproven(NO_KEY))?;
    let ready = unproved(&envelope, &method(here.domain, &public), created).map_err(unread)?;
    let signature = serving
        .ports
        .signer
        .sign(Algorithm::Ed25519, key, ready.message())
        .await
        .map_err(unproven)?;
    Ok((ready.proved(&signature).to_string().into_bytes(), public))
}

/// What an envelope this herald, served `here`, proved with the key whose
/// public half is `public` is held to: that key alone, and no other
/// instance an envelope may come from.
fn trusted<S, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    here: &Instance<'_>,
    public: PublicKey,
    now: Timestamp,
) -> Trust {
    let bounds = bounds(serving.config, now);
    Trust {
        url: format!("https://{}{}", here.domain, here.base),
        domain: here.domain.to_owned(),
        keys: vec![public],
        origins: Vec::new(),
        most: Peer::MOST,
        hops: 1,
        fields: bounds.fields,
        skew: bounds.skew,
        longest: bounds.longest,
    }
}

/// An envelope this herald cannot prove, as `error` says.
fn unproven(error: impl std::fmt::Display) -> Verdict {
    Verdict::Unavailable(format!("{NO_KEY}: {error}"))
}

/// A body that is no envelope, as `error` says.
fn unread(error: impl std::fmt::Display) -> Verdict {
    Verdict::Forbidden(format!("the body is no envelope: {error}"))
}
