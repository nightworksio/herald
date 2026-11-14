//! Forge webhooks: each verified with its source's secret and admitted once
//! before it is read, and the release it publishes left to announce
//! (ADR-0007, ADR-0013).

use std::collections::BTreeMap;

use herald_core::SourceName;
use herald_core::status;
use herald_kinds::Hooked;
use herald_ports::{Clock, Secret, Store, Verifier};

use super::jobs::Job;
use super::replies::{Answered, missing, rejection, text_reply, unread};
use super::{Inbound, Serving};
use crate::verification::{Checking, admitted, verified};

/// The secret each forge source's webhooks are signed with, by the source.
#[derive(Debug, Clone, Default)]
pub struct Webhooks(pub(crate) BTreeMap<SourceName, Secret>);

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
        .zip(serving.webhooks.0.get(&source))
        .and_then(|(hooking, secret)| hooking.scheme().map(|scheme| (hooking, scheme, secret)))
    else {
        return missing("webhook of such a source");
    };
    let store = serving.ports.store;
    let now = serving.ports.clock.now();
    let checked = verified(
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
    .await;
    let verified = match checked {
        Ok(verified) => verified,
        Err(rejected) => return rejection(&rejected),
    };
    if let Err(rejected) = admitted(store, serving.admission, scheme, &verified, now).await {
        return rejection(&rejected);
    }
    match hooking.hooked(inbound.headers, inbound.body) {
        Ok(Hooked::Event(event)) => Answered {
            reply: text_reply(status::ACCEPTED, "herald announces the release").reply,
            refused: None,
            job: Some(Job::Announce {
                source,
                event,
                scheme,
                verified,
            }),
        },
        Ok(Hooked::Nothing(why)) => text_reply(status::OK, why),
        Err(error) => unread(store, scheme, &verified, error).await,
    }
}
