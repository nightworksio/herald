//! Another herald's feed pulled (ADR-0049): the keys that stand for its
//! peer worked out, a key set the peer handed over to taken where its
//! rotation is signed, and the feed read through them.

use herald_config::{Config, Federation, Peer, Rotation};
use herald_core::{Cursor, PeerKeys, SourceName};
use herald_kinds::{Origin, Peering, PublicKey, Pull, Pulling, Trust, Unpulled};
use herald_peers::keys::KeySet;
use herald_peers::multibase::{ed25519_key, ed25519_public};
use herald_ports::{
    Clock, DynVerifier, Erased, Http, Published, Signer, Store, Versioned, Written,
};
use jiff::Timestamp;

use super::{Found, Sweep, announced_found, stored};
use crate::announce::{Outcome, Ports, Stop, Within};
use crate::peering::{Reading, bounds, handed_over};
use crate::prepare::Connected;
use crate::withdraw::tombstoned;

/// What `pull` lists since `cursor` through `http`, each answer and
/// envelope checked through `verifier` against the keys that stand for its
/// peer, each identity a tombstone withdraws withdrawn from every route
/// taking `source`, and each event it has not seen announced as `source`'s
/// under `holder`; nothing where it says nothing changed. Where an answer
/// is signed by a key not trusted and the peer's rotation is signed, the
/// peer's key set is read again and the feed asked once more. The cursor
/// stays where a withdrawal stopped.
pub(crate) async fn found<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (source, within): (&SourceName, Within<'_>),
    (pull, verifier, http): (&dyn Pull, &dyn DynVerifier, &H),
    (cursor, backfill): (Option<&Cursor>, usize),
    holder: &str,
) -> Result<Option<Found>, Stop> {
    let now = ports.clock.now();
    let reach = Reach {
        config,
        store: ports.store,
        http,
        verifier,
        now,
    };
    let pulling = Pulling {
        http,
        verifier,
        specs: &config.kinds,
        now,
    };
    let trust = reach.trusted(pull.peering(), Renew::Due).await?;
    let polled = match pull.pulled(pulling, &trust, cursor).await {
        Err(Unpulled::Stranger(_)) if signed(config, pull.peering()) => {
            let renewed = reach.trusted(pull.peering(), Renew::Now).await?;
            pull.pulled(pulling, &renewed, cursor).await
        }
        pulled => pulled,
    }
    .map_err(unpulled)?;
    let mut withdrawn = Vec::new();
    let mut settled = true;
    for identity in polled.iter().flat_map(|polled| &polled.withdrawn) {
        let announced = Box::pin(tombstoned(
            config,
            connected,
            ports,
            (
                source,
                identity,
                cursor.and_then(|cursor| cursor.project_of(identity)),
            ),
            holder,
        ))
        .await;
        settled &= !announced
            .iter()
            .any(|route| matches!(route.outcome, Outcome::Stopped { .. }));
        withdrawn.push(Sweep::Withdrawn {
            identity: identity.clone(),
            announced,
        });
    }
    if let Some(polled) = &polled {
        withdrawn.extend(polled.refused.iter().cloned().map(Sweep::Refused));
        withdrawn.extend(polled.ahead.iter().cloned().map(Sweep::Ahead));
    }
    let found = announced_found(
        (config, connected, ports),
        (source, within, None),
        (polled, pull.remembered()),
        (cursor, None, backfill),
        holder,
    )
    .await?;
    Ok(found.map(|mut found| {
        if !settled {
            found.advanced = None;
        }
        found.withdrawn = withdrawn;
        found
    }))
}

/// The peer of `[peers]` holding the key whose id is `keyid`, its table and the key,
/// each peer's keys those that stand for it as kept, never read again
/// (ADR-0054).
///
/// # Errors
///
/// A peer's kept key set cannot be read.
pub(crate) async fn signer<'c, S: Store, H: Http>(
    (config, store, http): (&'c Config, &S, &H),
    (verifier, now): (&dyn DynVerifier, Timestamp),
    keyid: &str,
) -> Result<Option<(&'c str, &'c Peer, PublicKey)>, Stop> {
    let reach = Reach {
        config,
        store,
        http,
        verifier,
        now,
    };
    for (name, peer) in config
        .peers
        .iter()
        .filter(|(_, peer)| keyid.contains(&peer.domain))
    {
        let keys = reach.standing(name, peer, Renew::Never).await?;
        if let Some(key) = keys
            .into_iter()
            .find(|key| herald_peers::keys::method(&peer.domain, key) == keyid)
        {
            return Ok(Some((name.as_str(), peer, key)));
        }
    }
    Ok(None)
}

/// What an envelope the peer `pusher` pushed is held to: the keys that
/// stand for it and for every other peer as kept, never read again
/// (ADR-0054).
///
/// # Errors
///
/// The pusher is no peer, or a peer's kept key set cannot be read.
pub(crate) async fn pushing<S: Store, H: Http>(
    (config, store, http): (&Config, &S, &H),
    (verifier, now): (&dyn DynVerifier, Timestamp),
    pusher: &str,
) -> Result<Trust, Stop> {
    Reach {
        config,
        store,
        http,
        verifier,
        now,
    }
    .trusted(Peering::Peer(pusher), Renew::Never)
    .await
}

/// Whether `peering` names a peer whose rotation is signed.
fn signed(config: &Config, peering: Peering<'_>) -> bool {
    matches!(peering, Peering::Peer(name) if config.peers.get(name).is_some_and(|peer| peer.rotation == Rotation::Signed))
}

/// Where a pull stops, as `unpulled` says.
fn unpulled(unpulled: Unpulled) -> Stop {
    match unpulled {
        Unpulled::Unreachable(why) => Stop::Unreachable(why),
        other => Stop::Source(other.to_string()),
    }
}

/// When the key set a peer handed over to is read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Renew {
    /// Never: the peer is reached through another `Http`.
    Never,
    /// Once it is due.
    Due,
    /// Now.
    Now,
}

/// What a pull reaches its peer's keys through in one round.
struct Reach<'a, S, H> {
    config: &'a Config,
    store: &'a S,
    http: &'a H,
    verifier: &'a dyn DynVerifier,
    now: Timestamp,
}

impl<S: Store, H: Http> Reach<'_, S, H> {
    /// What a pull of `peering`'s feed trusts: the keys that stand for its
    /// peer, read again first as `renew` says where its rotation is
    /// signed, and every other peer of `[peers]` an envelope it relays may
    /// come from, with the keys that stand for each as kept.
    async fn trusted(&self, peering: Peering<'_>, renew: Renew) -> Result<Trust, Stop> {
        let bounds = bounds(self.config, self.now);
        let (url, domain, keys, most, hops, name) = match peering {
            Peering::Pinned(instance) => (
                instance.url,
                instance.domain,
                self.unrevoked(instance.keys),
                Peer::MOST,
                Peer::HOPS,
                None,
            ),
            Peering::Peer(name) => {
                let peer = self.config.peers.get(name).ok_or_else(|| {
                    Stop::Source(format!("there is no peer {name} in [peers] to pull from"))
                })?;
                let keys = self.standing(name, peer, renew).await?;
                (
                    peer.url.as_str(),
                    peer.domain.as_str(),
                    keys,
                    peer.most,
                    peer.hops,
                    Some(name),
                )
            }
        };
        let mut origins = Vec::new();
        for (other, peer) in &self.config.peers {
            if Some(other.as_str()) != name {
                origins.push(Origin {
                    name: other.clone(),
                    url: peer.url.clone(),
                    domain: peer.domain.clone(),
                    keys: self.standing(other, peer, Renew::Never).await?,
                });
            }
        }
        Ok(Trust {
            url: url.to_owned(),
            domain: domain.to_owned(),
            keys,
            origins,
            most,
            hops,
            fields: bounds.fields,
            skew: bounds.skew,
            longest: bounds.longest,
        })
    }

    /// `keys` but those `[federation] revoked` lists, which stand for no
    /// one.
    fn unrevoked(&self, keys: &[PublicKey]) -> Vec<PublicKey> {
        keys.iter()
            .filter(|key| !self.config.federation.revoked.contains(key))
            .copied()
            .collect()
    }

    /// The keys that stand for the peer `name`: those pinned for it, or
    /// where its rotation is signed, the set it last handed over to while
    /// those stay pinned and it names no revoked key, read again first as
    /// `renew` says, and at once where it names a revoked key; never a
    /// revoked key.
    async fn standing(
        &self,
        name: &str,
        peer: &Peer,
        renew: Renew,
    ) -> Result<Vec<PublicKey>, Stop> {
        let Some(kept) = self
            .store
            .published()
            .filter(|_| peer.rotation == Rotation::Signed)
        else {
            return Ok(self.unrevoked(&peer.keys));
        };
        let pinned: Vec<String> = peer.keys.iter().map(ed25519_public).collect();
        let held = kept.peer_keys(name).await.map_err(stored)?;
        let standing = held
            .as_ref()
            .filter(|held| held.value.stands(&pinned))
            .and_then(|held| keys(&held.value).map(|keys| (held, keys)))
            .filter(|(_, keys)| self.unrevoked(keys) == *keys);
        let due = |held: &Versioned<PeerKeys>| match renew {
            Renew::Never => false,
            Renew::Due => held.value.due(self.now),
            Renew::Now => true,
        };
        match standing {
            Some((held, keys)) if !due(held) => Ok(keys),
            None if renew == Renew::Never => Ok(self.unrevoked(&peer.keys)),
            Some((held, keys)) => {
                let set = KeySet {
                    sequence: held.value.sequence,
                    refresh: held.value.refresh,
                    keys,
                };
                self.renewed((kept, name, peer), set, Some(held)).await
            }
            None => {
                let set = KeySet {
                    sequence: 0,
                    refresh: Federation::REFRESH.as_secs().unsigned_abs(),
                    keys: self.unrevoked(&peer.keys),
                };
                self.renewed((kept, name, peer), set, held.as_ref()).await
            }
        }
    }

    /// The keys that stand for the peer `name` once its discovery document
    /// is read again: the set it hands over to from `held`, where a key of
    /// `held` proved it and it names no revoked key, else `held`'s own,
    /// kept in the store with when it was read.
    async fn renewed<P: Published>(
        &self,
        (kept, name, peer): (&P, &str, &Peer),
        held: KeySet,
        record: Option<&Versioned<PeerKeys>>,
    ) -> Result<Vec<PublicKey>, Stop> {
        let discovery = peer.discovery();
        let reading = Reading {
            url: &discovery,
            domain: &peer.domain,
            most: peer.most,
            bounds: bounds(self.config, self.now),
        };
        let taken = match handed_over(self.http, &Erased(self.verifier), reading, &held).await {
            Ok(Some(set))
                if !set
                    .keys
                    .iter()
                    .any(|key| self.config.federation.revoked.contains(key)) =>
            {
                set
            }
            _ => held,
        };
        let longest = Federation::REFRESH_ALLOWED.as_secs().unsigned_abs();
        let written = PeerKeys {
            pinned: peer.keys.iter().map(ed25519_public).collect(),
            sequence: taken.sequence,
            keys: taken.keys.iter().map(ed25519_public).collect(),
            read: self.now,
            refresh: taken.refresh.min(longest),
        };
        match kept
            .put_peer_keys(name, &written, record.map(|record| record.version))
            .await
        {
            Ok(Written::Written(_) | Written::Conflict) => Ok(taken.keys),
            Err(error) => Err(stored(error)),
        }
    }
}

/// The keys `held` lists, where each is an Ed25519 Multikey.
fn keys(held: &PeerKeys) -> Option<Vec<PublicKey>> {
    held.keys.iter().map(|key| ed25519_key(key)).collect()
}
