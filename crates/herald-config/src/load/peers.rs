//! `[peers.*]`: each peer's address, pinned keys and bounds checked, and no
//! key pinned for two peers.

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::SourceName;
use herald_kinds::{Peering, Source};

use herald_core::{Grant, Size, kind_name};
use herald_peers::discovery;
use herald_peers::keys::Key;
use herald_peers::multibase::{ed25519_key, ed25519_public};

use super::oidc::Standing;
use super::{Loader, Value};
use crate::file::PeerFile;
use crate::peers::{Peer, Rotation};

impl Loader<'_> {
    /// Each source pulling another herald's feed joined to the peer it
    /// names, refused where `[peers]` has no such peer; one pinning keys of
    /// its own refused where it pins a key `revoked` lists or a peer pins
    /// (ADR-0049).
    pub(super) fn joined(
        &mut self,
        (sources, spans): (
            &mut BTreeMap<SourceName, Source>,
            &BTreeMap<SourceName, Range<usize>>,
        ),
        peers: &BTreeMap<String, Peer>,
        revoked: &[Key],
    ) {
        for (name, source) in sources.iter_mut() {
            let span = spans.get(name).cloned().unwrap_or_default();
            let path = format!("sources.{}", name.as_str());
            match source.peering() {
                Some(Peering::Peer(peer)) => match peers.get(peer) {
                    Some(found) => {
                        if source.stale_after().is_some() && found.rotation == Rotation::Signed {
                            self.error(
                                &span,
                                &format!("{path}.static"),
                                format!("a static log is served with no signed answer, which a signed rotation is read from; set rotation = \"manual\" in [peers.{peer}], or read the peer's herald server"),
                            );
                        }
                        if let Some(joined) = source.joined(&found.url) {
                            *source = joined;
                        }
                    }
                    None => self.error(
                        &span,
                        &format!("{path}.peer"),
                        format!("there is no peer \"{peer}\" in [peers]; `herald peer add <url>` pins one"),
                    ),
                },
                Some(Peering::Pinned(instance)) => {
                    for key in instance.keys {
                        let shown = ed25519_public(key);
                        if revoked.contains(key) {
                            self.error(
                                &span,
                                &format!("{path}.keys"),
                                format!("the key {shown} is revoked in [federation] revoked; pin a key the instance has not lost"),
                            );
                        }
                        if let Some((peer, _)) = peers.iter().find(|(_, peer)| peer.keys.contains(key)) {
                            self.error(
                                &span,
                                &format!("{path}.keys"),
                                format!("the key {shown} is pinned for the peer {peer}; name it as peer = \"{peer}\" in place of url and keys"),
                            );
                        }
                    }
                }
                None => {}
            }
        }
    }

    /// Each peer of `[peers]`, by name.
    pub(super) fn peers(
        &mut self,
        value: Option<Value<'_>>,
        revoked: &[Key],
    ) -> BTreeMap<String, Peer> {
        let mut peers = BTreeMap::new();
        let mut pinned: BTreeMap<Key, String> = BTreeMap::new();
        for (name, value) in self.named(value, "peers") {
            let path = format!("peers.{name}");
            let span = value.span();
            let Some(file) = self.typed::<PeerFile>(value, &path) else {
                continue;
            };
            if !kind_name(&name) {
                self.error(
                    &span,
                    &path,
                    format!("\"{name}\" names the peer in its events' identities; give a name of lowercase letters, digits and inner '-'"),
                );
                continue;
            }
            if let Some(peer) = self.peer(&file, &span, &path) {
                for key in &peer.keys {
                    if revoked.contains(key) {
                        self.error(
                            &span,
                            &format!("{path}.keys"),
                            format!("the key {} is revoked in [federation] revoked; pin a key the peer has not lost", ed25519_public(key)),
                        );
                    }
                    if let Some(other) = pinned.insert(*key, name.clone()) {
                        self.error(
                            &span,
                            &format!("{path}.keys"),
                            format!("a key is pinned for both {other} and {name}; a key stands for one peer, so pin it for one"),
                        );
                    }
                }
                peers.insert(name, peer);
            }
        }
        peers
    }

    /// The peer `file` describes, at `path`; `None` where it does not load.
    fn peer(&mut self, file: &PeerFile, span: &Range<usize>, path: &str) -> Option<Peer> {
        let at = |key: &str| format!("{path}.{key}");
        let address = discovery::address(&file.url);
        if address.is_none() {
            self.error(
                span,
                &at("url"),
                format!("\"{}\" is not a peer's address; give https://, its domain and any path its endpoints are under, such as \"https://herald.example\", with no port, query or fragment", file.url),
            );
        }
        let mut keys = Vec::with_capacity(file.keys.len());
        for written in &file.keys {
            match ed25519_key(written) {
                Some(key) if keys.contains(&key) => {
                    self.error(
                        span,
                        &at("keys"),
                        format!("\"{written}\" is pinned twice; pin each key once"),
                    );
                }
                Some(key) => keys.push(key),
                None => {
                    self.error(
                        span,
                        &at("keys"),
                        format!("\"{written}\" is not an Ed25519 Multikey; pin the keys `herald peer add` shows, each such as \"z6Mk…\""),
                    );
                }
            }
        }
        if file.keys.is_empty() {
            self.error(
                span,
                &at("keys"),
                "pin at least one of the peer's keys; `herald peer add <url>` shows them and pins those you confirm",
            );
        }
        let rate = file.rate.unwrap_or(Peer::RATE);
        if rate.exceeds(&Peer::RATE_ALLOWED) {
            self.error(
                span,
                &at("rate"),
                format!("rate is at most \"{}\"; lower it", Peer::RATE_ALLOWED),
            );
        }
        let most = file.most.map_or(Peer::MOST, |most| most.0);
        if most > Peer::MOST_ALLOWED {
            self.error(
                span,
                &at("most"),
                format!(
                    "most is at most {}; lower it",
                    Size::written(Peer::MOST_ALLOWED)
                ),
            );
        }
        let hops = file.hops.unwrap_or(Peer::HOPS);
        if !(1..=Peer::HOPS_ALLOWED).contains(&hops) {
            self.error(
                span,
                &at("hops"),
                format!("hops is from 1 to {}", Peer::HOPS_ALLOWED),
            );
        }
        let grants = self.peer_grants(file, span, path);
        let (url, domain) = address?;
        Some(Peer {
            url,
            domain,
            keys,
            rotation: file.rotation.unwrap_or_default(),
            rate,
            most,
            hops,
            private: file.private.unwrap_or_default(),
            grants,
        })
    }

    /// The grants `file` lists, at `path`, by the source each names; one
    /// naming no source, granting nothing, or naming a source twice is
    /// refused.
    fn peer_grants(
        &mut self,
        file: &PeerFile,
        span: &Range<usize>,
        path: &str,
    ) -> BTreeMap<SourceName, Grant> {
        let mut grants = BTreeMap::new();
        for (at, written) in file.grants.iter().enumerate() {
            let here = format!("{path}.grants[{at}]");
            let Some(source) = written.resource.strip_prefix(Peer::SOURCES) else {
                self.error(
                    span,
                    &format!("{here}.resource"),
                    format!("\"{}\" is no resource a peer may be granted; name a herald source, as \"{}<name>\"", written.resource, Peer::SOURCES),
                );
                continue;
            };
            if written.actions.is_empty() {
                self.error(
                    span,
                    &format!("{here}.actions"),
                    "the grant grants no action; grant one, such as actions = [\"push\"]",
                );
                continue;
            }
            let grant = Grant {
                actions: written.actions.clone(),
                until: written.until,
                namespaces: written.namespaces.clone(),
            };
            if grants.insert(SourceName::new(source), grant).is_some() {
                self.error(
                    span,
                    &format!("{here}.resource"),
                    format!(
                        "{} is granted twice; grant each resource once",
                        written.resource
                    ),
                );
            }
        }
        grants
    }

    /// Each grant checked against the resource it names: a peer's grant
    /// and the source's own must both exist and say the same, or the
    /// config does not load, naming both places. A name an app stands for,
    /// of `standing`, is granted by the source alone, and no source grants
    /// one app two names, which a push could not tell apart (ADR-0064).
    pub(super) fn granted(
        &mut self,
        (sources, spans): (
            &BTreeMap<SourceName, Source>,
            &BTreeMap<SourceName, Range<usize>>,
        ),
        peers: &BTreeMap<String, Peer>,
        standing: &Standing<'_>,
    ) {
        for (name, peer) in peers {
            for (source, grant) in &peer.grants {
                let path = format!("peers.{name}.grants");
                let resource = format!("{}{}", Peer::SOURCES, source.as_str());
                let span = spans.get(source).cloned().unwrap_or_default();
                match sources.get(source).map(|found| found.grants()) {
                    None => self.error(
                        &span,
                        &path,
                        format!("there is no source {resource} to grant; name a herald source"),
                    ),
                    Some(None) => self.error(
                        &span,
                        &path,
                        format!("{resource} takes no grants; only a herald source takes what a peer pushes"),
                    ),
                    Some(Some(granted)) if granted.get(name) != Some(grant) => self.error(
                        &span,
                        &path,
                        format!("{resource}.peers.{name} must grant the same as the grant for {resource} in [peers.{name}]; make both name the same actions, until and namespaces"),
                    ),
                    Some(Some(_)) => {}
                }
            }
        }
        for (source, found) in sources {
            let span = spans.get(source).cloned().unwrap_or_default();
            let mut apps: BTreeMap<&str, &str> = BTreeMap::new();
            for name in found.grants().into_iter().flat_map(BTreeMap::keys) {
                let resource = format!("{}{}", Peer::SOURCES, source.as_str());
                let at = format!("{resource}.peers.{name}");
                if let Some(app) = standing.names.get(name.as_str()) {
                    if let Some(other) = apps.insert(app, name) {
                        self.error(
                            &span,
                            &at,
                            format!("apps.{app} stands for \"{other}\" too, which {resource} grants as well, so a push of the app's could not tell which it is; grant one name of each app"),
                        );
                    }
                } else if !peers.contains_key(name) {
                    self.error(
                        &span,
                        &at,
                        format!("there is no peer \"{name}\" in [peers], and no app stands for it; name a table of [peers], or an app's peers"),
                    );
                } else if !peers
                    .get(name)
                    .is_some_and(|peer| peer.grants.contains_key(source))
                {
                    self.error(
                        &span,
                        &at,
                        format!("[peers.{name}] grants nothing on {resource}; add {{ resource = \"{resource}\", ... }} to its grants, the same as here"),
                    );
                }
            }
        }
    }
}
