//! `[peers.*]`: each peer's address, pinned keys and bounds checked, and no
//! key pinned for two peers.

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::{Size, kind_name};
use herald_peers::keys::Key;
use herald_peers::multibase::{ed25519_key, ed25519_public};

use super::{Loader, Value};
use crate::file::PeerFile;
use crate::peers::Peer;

impl Loader<'_> {
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
        let address = Peer::address(&file.url);
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
        })
    }
}
