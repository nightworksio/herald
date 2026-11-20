//! What doctor shows of the instance among other heralds (ADR-0058): its
//! domain and keys, each peer it trusts, each feed and push it makes, each
//! herald source it reads into, the grants peers hold, the ID tokens apps
//! take (ADR-0064), and what of it this layer cannot serve.

use std::fmt;

use herald_config::{Config, Pages, Peer};
use herald_core::{Grant, Size, SourceName, itself};
use herald_keys::ed25519_of;
use herald_kinds::Peering;
use herald_peers::discovery::{Instance, inbox_of};
use herald_peers::keys::{Key, fingerprint};
use herald_peers::multibase::ed25519_public;
use herald_ports::{Algorithm, Secrets, Signer};
use jiff::{SignedDuration, Timestamp};

use super::{Gap, Layer};

/// An Ed25519 public key as an operator compares it: its Multikey and its
/// fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// Its Multikey, such as `z6Mk…`.
    pub key: String,
    /// Its fingerprint, `sha256:` and hex.
    pub fingerprint: String,
}

impl Shown {
    /// `key` as shown.
    #[must_use]
    pub fn of(key: &Key) -> Self {
        Self {
            key: ed25519_public(key),
            fingerprint: fingerprint(key),
        }
    }
}

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.key, self.fingerprint)
    }
}

/// One of the instance's own keys: the secret holding it, and the key,
/// where the secret is set here and holds an Ed25519 key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnKey {
    /// The secret's name.
    pub secret: String,
    /// The public key, where it is read.
    pub shown: Option<Shown>,
}

/// Whether a peer's discovery document was read, as `--online` asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reached {
    /// It was read, and signed by a key pinned for the peer.
    Read,
    /// It was not: why.
    Unread(String),
}

/// A peer of `[peers]` as doctor shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerShown {
    /// Its name.
    pub name: String,
    /// Where it is served.
    pub url: String,
    /// The keys pinned for it.
    pub keys: Vec<Shown>,
    /// How its keys rotate, as `rotation` names it.
    pub rotation: &'static str,
    /// How many requests it may make, as `rate` writes it.
    pub rate: String,
    /// The most bytes of a body it sends that are read, as `most` writes
    /// it.
    pub most: String,
    /// Whether private events may go to it.
    pub private: bool,
    /// Whether its discovery document was read; `None` offline.
    pub reached: Option<Reached>,
}

/// A feed of herald's own as doctor shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedShown {
    /// The destination's name, which is the feed's.
    pub name: String,
    /// Its URL, on `[federation]`'s domain.
    pub url: String,
    /// Whether the layer serves it.
    pub served: bool,
}

/// A `herald-push` destination as doctor shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushShown {
    /// The destination's name.
    pub name: String,
    /// The peer it pushes to.
    pub peer: String,
    /// The URL of the inbox it pushes into.
    pub inbox: String,
    /// Whether private events go to the peer.
    pub private: bool,
    /// Whether it presents a CI run's ID token in place of the instance's
    /// key (ADR-0064).
    pub id_token: bool,
}

/// An issuer of ID tokens an app takes, as doctor shows it (ADR-0064).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunShown {
    /// The app's name.
    pub app: String,
    /// The issuer.
    pub issuer: String,
    /// What a token's audience must name.
    pub audience: String,
    /// The names the app stands for when its runs push.
    pub peers: Vec<String>,
}

/// Where a `herald` source reads from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pulls {
    /// The feed of the peer of this name.
    Peer(String),
    /// The instance served at this URL, which the source pins itself.
    Pinned(String),
}

/// A `herald` source as doctor shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceShown {
    /// The source's name.
    pub name: String,
    /// Whose feed it pulls, where it pulls one.
    pub pulls: Option<Pulls>,
    /// Whether a peer may push into it.
    pub pushed: bool,
    /// How long after the static log it reads was written it is still
    /// taken, where it reads one (ADR-0060).
    pub stale_after: Option<SignedDuration>,
}

/// What a peer may do with a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantShown {
    /// The peer's name.
    pub peer: String,
    /// The source's name.
    pub source: String,
    /// What it may do, such as `push`.
    pub actions: Vec<&'static str>,
    /// When the grant ends; never where `None`.
    pub until: Option<Timestamp>,
}

/// The instance among other heralds, as doctor shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Federation {
    /// The domain, where `[federation]` names it.
    pub domain: Option<String>,
    /// Its own keys, newest first.
    pub keys: Vec<OwnKey>,
    /// The peer keys never trusted again.
    pub revoked: Vec<Shown>,
    /// Each peer.
    pub peers: Vec<PeerShown>,
    /// Each feed of herald's own.
    pub feeds: Vec<FeedShown>,
    /// Each push to a peer.
    pub pushes: Vec<PushShown>,
    /// Each `herald` source.
    pub sources: Vec<SourceShown>,
    /// What each peer may do with each source.
    pub grants: Vec<GrantShown>,
    /// Each issuer of ID tokens each app takes.
    pub runs: Vec<RunShown>,
    /// How long what each peer asked is kept.
    pub keep_audit: SignedDuration,
    /// Where its feeds are written as a static signed log, where they are
    /// (ADR-0060).
    pub pages: Option<Pages>,
    /// The index of each route to a feed or a push.
    pub routes: Vec<usize>,
    /// What of it the layer cannot serve.
    pub gaps: Vec<Gap>,
}

/// Whether `key` names `name` itself or a key under it.
fn under(key: &str, name: &str) -> bool {
    key.strip_prefix(name)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.') || rest.starts_with('['))
}

impl Federation {
    /// Whether the gap at `key` is one of federation's: of `[federation]`,
    /// a peer, a feed, a push, a route to either or a `herald` source.
    #[must_use]
    pub fn owns(&self, key: &str) -> bool {
        under(key, "federation")
            || self
                .peers
                .iter()
                .any(|peer| under(key, &format!("peers.{}", peer.name)))
            || self
                .feeds
                .iter()
                .map(|feed| &feed.name)
                .chain(self.pushes.iter().map(|push| &push.name))
                .any(|name| under(key, &format!("destinations.{name}")))
            || self
                .sources
                .iter()
                .any(|source| under(key, &format!("{}{}", Peer::SOURCES, source.name)))
            || self
                .routes
                .iter()
                .any(|index| under(key, &format!("routes[{index}]")))
            || self
                .runs
                .iter()
                .any(|run| under(key, &format!("apps.{}", run.app)))
    }

    /// Whether the config takes any part among other heralds.
    #[must_use]
    pub fn any(&self) -> bool {
        self.domain.is_some()
            || !self.keys.is_empty()
            || !self.peers.is_empty()
            || !self.feeds.is_empty()
            || !self.pushes.is_empty()
            || !self.sources.is_empty()
            || !self.runs.is_empty()
            || self.pages.is_some()
    }
}

/// The instance among other heralds as `config` makes it on `layer`: its
/// own keys read through `secrets` and `signer`, and of `gaps`, those that
/// are federation's; `None` where it takes no part among them.
pub async fn federation<S: Secrets, G: Signer>(
    config: &Config,
    layer: &Layer,
    (secrets, signer): (&S, &G),
    gaps: &[Gap],
) -> Option<Federation> {
    let mut keys = Vec::with_capacity(config.federation.keys.len());
    for reference in &config.federation.keys {
        keys.push(OwnKey {
            secret: reference.secret.clone(),
            shown: own(secrets, signer, &reference.secret).await,
        });
    }
    let mut shown = Federation {
        domain: config.federation.domain.clone(),
        keys,
        revoked: config.federation.revoked.iter().map(Shown::of).collect(),
        peers: peers(config),
        feeds: feeds(config, layer),
        pushes: pushes(config),
        sources: sources(config),
        grants: grants(config),
        runs: runs(config),
        keep_audit: config.federation.keep_audit,
        pages: config.federation.pages.clone(),
        routes: Vec::new(),
        gaps: Vec::new(),
    };
    shown.routes = config
        .routes
        .iter()
        .enumerate()
        .filter(|(_, route)| {
            shown
                .feeds
                .iter()
                .map(|feed| feed.name.as_str())
                .chain(shown.pushes.iter().map(|push| push.name.as_str()))
                .any(|name| route.route.destination.as_str() == name)
        })
        .map(|(index, _)| index)
        .collect();
    shown.gaps = gaps
        .iter()
        .filter(|gap| shown.owns(&gap.key))
        .cloned()
        .collect();
    shown.any().then_some(shown)
}

/// The public key the secret `name` holds, where it is set here and holds
/// an Ed25519 key.
async fn own<S: Secrets, G: Signer>(secrets: &S, signer: &G, name: &str) -> Option<Shown> {
    let secret = secrets.secret(name).await.ok()?;
    let spki = signer.public(Algorithm::Ed25519, &secret).await.ok()?;
    ed25519_of(&spki).map(|key| Shown::of(&key))
}

/// Each peer, offline.
fn peers(config: &Config) -> Vec<PeerShown> {
    config
        .peers
        .iter()
        .map(|(name, peer)| PeerShown {
            name: name.clone(),
            url: peer.url.clone(),
            keys: peer.keys.iter().map(Shown::of).collect(),
            rotation: peer.rotation.name(),
            rate: peer.rate.to_string(),
            most: Size::written(peer.most).trim_matches('"').to_owned(),
            private: peer.private,
            reached: None,
        })
        .collect()
}

/// Each feed of herald's own, and whether `layer` serves it.
fn feeds(config: &Config, layer: &Layer) -> Vec<FeedShown> {
    let instance = Instance {
        domain: config.federation.domain.as_deref().unwrap_or_default(),
        base: config.instance_base(),
        software: itself::NAME,
        version: itself::VERSION,
        repository: herald_core::repository!(),
    };
    config
        .destinations
        .iter()
        .filter(|(_, destination)| destination.feeding().is_some())
        .map(|(name, _)| FeedShown {
            name: name.as_str().to_owned(),
            url: instance.feed(name.as_str()),
            served: layer.endpoint,
        })
        .collect()
}

/// Each push to a peer.
fn pushes(config: &Config) -> Vec<PushShown> {
    config
        .destinations
        .iter()
        .filter_map(|(name, destination)| {
            let pushing = destination.pushing()?;
            let peer = config.peers.get(pushing.peer)?;
            Some(PushShown {
                name: name.as_str().to_owned(),
                peer: pushing.peer.to_owned(),
                inbox: inbox_of(&peer.url, pushing.inbox),
                private: peer.private,
                id_token: pushing.id_token,
            })
        })
        .collect()
}

/// Each `herald` source: whose feed it pulls, and whether a peer may push
/// into it.
fn sources(config: &Config) -> Vec<SourceShown> {
    config
        .sources
        .iter()
        .filter(|(_, source)| source.grants().is_some())
        .map(|(name, source)| SourceShown {
            name: name.as_str().to_owned(),
            pulls: source.peering().map(|peering| match peering {
                Peering::Peer(peer) => Pulls::Peer(peer.to_owned()),
                Peering::Pinned(instance) => Pulls::Pinned(instance.url.to_owned()),
            }),
            pushed: source.receiver().is_some(),
            stale_after: source.stale_after(),
        })
        .collect()
}

/// What each peer, and each name an app stands for, may do with each
/// source.
fn grants(config: &Config) -> Vec<GrantShown> {
    let shown = |name: &str, source: &SourceName, grant: &Grant| GrantShown {
        peer: name.to_owned(),
        source: source.as_str().to_owned(),
        actions: grant.actions.iter().map(|action| action.name()).collect(),
        until: grant.until,
    };
    let peers = config.peers.iter().flat_map(|(name, peer)| {
        peer.grants
            .iter()
            .map(move |(source, grant)| shown(name, source, grant))
    });
    let stood = config.sources.iter().flat_map(|(source, found)| {
        found
            .grants()
            .into_iter()
            .flatten()
            .filter(|(name, _)| config.apps.values().any(|app| app.peers.contains(*name)))
            .map(move |(name, grant)| shown(name, source, grant))
    });
    peers.chain(stood).collect()
}

/// Each issuer of ID tokens each app takes.
fn runs(config: &Config) -> Vec<RunShown> {
    config
        .apps
        .iter()
        .flat_map(|(app, found)| {
            found.oidc.iter().map(move |entry| RunShown {
                app: app.clone(),
                issuer: entry.issuer.clone(),
                audience: entry.audience.clone(),
                peers: found.peers.iter().cloned().collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
