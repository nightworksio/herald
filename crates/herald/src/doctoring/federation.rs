//! doctor's federation section (ADR-0058): the instance among other
//! heralds, as lines and as the `federation` member of the JSON report,
//! each peer's discovery document read with `--online`.

use herald_config::Config;
use herald_flows::{
    Federation, FeedShown, GrantShown, OwnKey, PeerShown, Pulls, PushShown, Reached, Shown,
    SourceShown,
};
use schemars::JsonSchema;
use serde::Serialize;

use super::{Gapped, gapped_json};
use crate::Connect;
use crate::peers::read;

/// Each peer of `federation` with whether its discovery document is read
/// through `connect`, signed by a key pinned for it.
pub(super) async fn reached(config: &Config, connect: Connect<'_>, federation: &mut Federation) {
    for (shown, peer) in federation.peers.iter_mut().zip(config.peers.values()) {
        let read = read(
            config,
            connect,
            (&peer.discovery(), &peer.domain, peer.most),
            Some(&peer.keys),
        )
        .await;
        shown.reached = Some(read.map_or_else(Reached::Unread, |_| Reached::Read));
    }
}

/// An Ed25519 public key as an operator compares it.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct KeyJson<'a> {
    /// Its Multikey, such as `z6Mk…`.
    key: &'a str,
    /// Its fingerprint: `sha256:` and the hex of its SHA-256.
    fingerprint: &'a str,
}

impl<'a> From<&'a Shown> for KeyJson<'a> {
    fn from(shown: &'a Shown) -> Self {
        Self {
            key: &shown.key,
            fingerprint: &shown.fingerprint,
        }
    }
}

/// One of the instance's own keys.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct OwnKeyJson<'a> {
    /// The secret holding it.
    secret: &'a str,
    /// The public key; null where the secret is not set here or holds no
    /// Ed25519 key.
    #[schemars(required)]
    public: Option<KeyJson<'a>>,
}

/// Whether a peer's discovery document was read.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ReachedJson<'a> {
    /// Whether it was read, signed by a key pinned for the peer.
    read: bool,
    /// Why it was not; null where it was.
    #[schemars(required)]
    why: Option<&'a str>,
}

/// A peer of `[peers]`.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct PeerJson<'a> {
    /// Its name.
    name: &'a str,
    /// Where it is served.
    url: &'a str,
    /// The keys pinned for it.
    keys: Vec<KeyJson<'a>>,
    /// How its keys rotate: `manual` or `signed`.
    rotation: &'a str,
    /// How many requests it may make, as `rate` writes it.
    rate: &'a str,
    /// The most bytes of a body it sends that are read, as `most` writes it.
    most: &'a str,
    /// Whether private events may go to it.
    private: bool,
    /// Whether its discovery document was read; null unless `--online`.
    #[schemars(required)]
    discovery: Option<ReachedJson<'a>>,
}

/// A feed of herald's own.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct FeedJson<'a> {
    /// The destination's name, which is the feed's.
    name: &'a str,
    /// Its URL.
    url: &'a str,
    /// Whether the layer serves it.
    served: bool,
}

/// A `herald-push` destination.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct PushJson<'a> {
    /// The destination's name.
    name: &'a str,
    /// The peer it pushes to.
    peer: &'a str,
    /// The URL of the inbox it pushes into.
    inbox: &'a str,
    /// Whether private events go to the peer.
    private: bool,
}

/// A `herald` source.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct SourceJson<'a> {
    /// The source's name.
    name: &'a str,
    /// The peer whose feed it pulls; null where it pulls none of `[peers]`.
    #[schemars(required)]
    peer: Option<&'a str>,
    /// Where the instance it pins itself is served; null where it pins
    /// none.
    #[schemars(required)]
    pinned: Option<&'a str>,
    /// Whether a peer may push into it.
    pushed: bool,
    /// How many seconds after the static log it reads was written it is
    /// still taken; null where it reads no static log.
    #[schemars(required)]
    stale_after: Option<i64>,
}

/// Where the instance's feeds are written as a static signed log.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct PagesJson<'a> {
    /// Where a static host serves it.
    url: &'a str,
    /// The branch each writing is committed to; null where it is committed
    /// to none.
    #[schemars(required)]
    branch: Option<&'a str>,
    /// The directory each writing fills; null where it fills none.
    #[schemars(required)]
    artifact: Option<&'a str>,
    /// How many seconds a log no feed changed is left before it is written
    /// again.
    refresh: i64,
}

/// What a peer may do with a source.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct GrantJson<'a> {
    /// The peer's name.
    peer: &'a str,
    /// The source's name.
    source: &'a str,
    /// What it may do, such as `push`.
    actions: &'a [&'static str],
    /// When it ends, in RFC 3339; null where it never does.
    #[schemars(required)]
    until: Option<String>,
}

/// The instance among other heralds.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct FederationJson<'a> {
    /// The domain; null where `[federation]` names none.
    #[schemars(required)]
    domain: Option<&'a str>,
    /// Its own keys, newest first.
    keys: Vec<OwnKeyJson<'a>>,
    /// The peer keys never trusted again.
    revoked: Vec<KeyJson<'a>>,
    /// Each peer.
    peers: Vec<PeerJson<'a>>,
    /// Each feed of herald's own.
    feeds: Vec<FeedJson<'a>>,
    /// Each push to a peer.
    pushes: Vec<PushJson<'a>>,
    /// Each `herald` source.
    sources: Vec<SourceJson<'a>>,
    /// What each peer may do with each source.
    grants: Vec<GrantJson<'a>>,
    /// How many seconds what each peer asked is kept.
    keep_audit: i64,
    /// Where its feeds are written as a static signed log; null where they
    /// are not.
    #[schemars(required)]
    pages: Option<PagesJson<'a>>,
    /// What of it the layer cannot serve.
    gaps: Vec<Gapped<'a>>,
}

impl<'a> From<&'a Federation> for FederationJson<'a> {
    fn from(federation: &'a Federation) -> Self {
        Self {
            domain: federation.domain.as_deref(),
            keys: federation.keys.iter().map(own_json).collect(),
            revoked: federation.revoked.iter().map(KeyJson::from).collect(),
            peers: federation.peers.iter().map(peer_json).collect(),
            feeds: federation.feeds.iter().map(feed_json).collect(),
            pushes: federation.pushes.iter().map(push_json).collect(),
            sources: federation.sources.iter().map(source_json).collect(),
            grants: federation.grants.iter().map(grant_json).collect(),
            keep_audit: federation.keep_audit.as_secs(),
            pages: federation.pages.as_ref().map(|pages| PagesJson {
                url: &pages.url,
                branch: pages.branch.as_deref(),
                artifact: pages.artifact.as_deref(),
                refresh: pages.refresh.as_secs(),
            }),
            gaps: federation.gaps.iter().map(gapped_json).collect(),
        }
    }
}

/// `key` as the JSON names it.
fn own_json(key: &OwnKey) -> OwnKeyJson<'_> {
    OwnKeyJson {
        secret: &key.secret,
        public: key.shown.as_ref().map(KeyJson::from),
    }
}

/// `peer` as the JSON names it.
fn peer_json(peer: &PeerShown) -> PeerJson<'_> {
    PeerJson {
        name: &peer.name,
        url: &peer.url,
        keys: peer.keys.iter().map(KeyJson::from).collect(),
        rotation: peer.rotation,
        rate: &peer.rate,
        most: &peer.most,
        private: peer.private,
        discovery: peer.reached.as_ref().map(|reached| match reached {
            Reached::Read => ReachedJson {
                read: true,
                why: None,
            },
            Reached::Unread(why) => ReachedJson {
                read: false,
                why: Some(why),
            },
        }),
    }
}

/// `feed` as the JSON names it.
fn feed_json(feed: &FeedShown) -> FeedJson<'_> {
    FeedJson {
        name: &feed.name,
        url: &feed.url,
        served: feed.served,
    }
}

/// `push` as the JSON names it.
fn push_json(push: &PushShown) -> PushJson<'_> {
    PushJson {
        name: &push.name,
        peer: &push.peer,
        inbox: &push.inbox,
        private: push.private,
    }
}

/// `source` as the JSON names it.
fn source_json(source: &SourceShown) -> SourceJson<'_> {
    let (peer, pinned) = match &source.pulls {
        Some(Pulls::Peer(peer)) => (Some(peer.as_str()), None),
        Some(Pulls::Pinned(url)) => (None, Some(url.as_str())),
        None => (None, None),
    };
    SourceJson {
        name: &source.name,
        peer,
        pinned,
        pushed: source.pushed,
        stale_after: source.stale_after.map(|stale_after| stale_after.as_secs()),
    }
}

/// `grant` as the JSON names it.
fn grant_json(grant: &GrantShown) -> GrantJson<'_> {
    GrantJson {
        peer: &grant.peer,
        source: &grant.source,
        actions: &grant.actions,
        until: grant.until.map(|until| until.to_string()),
    }
}

/// The section as lines, after doctor's findings, which already name its
/// gaps and grants.
pub(super) fn lines(federation: &Federation) -> Vec<String> {
    let mut lines = vec![federation.domain.as_deref().map_or_else(
        || "Federation: [federation] names no domain.".to_owned(),
        |domain| format!("Federation: the instance is {domain}."),
    )];
    for (index, key) in federation.keys.iter().enumerate() {
        lines.push(key.shown.as_ref().map_or_else(
            || format!("federation.keys[{index}]: {} is not read here.", key.secret),
            |shown| format!("federation.keys[{index}]: {} holds {shown}.", key.secret),
        ));
    }
    for shown in &federation.revoked {
        lines.push(format!(
            "federation.revoked: {shown} is never trusted again."
        ));
    }
    lines.extend(federation.peers.iter().map(peer_line));
    for feed in &federation.feeds {
        let served = if feed.served { "served" } else { "not served" };
        lines.push(format!(
            "destinations.{}: the feed {}, {served} on this layer.",
            feed.name, feed.url
        ));
    }
    for push in &federation.pushes {
        let private = if push.private {
            "private events too"
        } else {
            "public events alone"
        };
        lines.push(format!(
            "destinations.{}: pushes to peers.{} at {}, {private}.",
            push.name, push.peer, push.inbox
        ));
    }
    lines.extend(federation.sources.iter().map(source_line));
    lines.push(format!(
        "federation.keep_audit: what each peer asks is kept {}.",
        herald_core::Duration(federation.keep_audit).written()
    ));
    if let Some(pages) = &federation.pages {
        let written: Vec<String> = pages
            .branch
            .iter()
            .map(|branch| format!("committed to the branch {branch}"))
            .chain(
                pages
                    .artifact
                    .iter()
                    .map(|artifact| format!("written to {artifact}")),
            )
            .collect();
        lines.push(format!(
            "federation.pages: the static signed log at {}, {}, written again unchanged after {}.",
            pages.url,
            written.join(" and "),
            herald_core::Duration(pages.refresh).written()
        ));
    }
    lines
}

/// `peer` as a line.
fn peer_line(peer: &PeerShown) -> String {
    let keys: Vec<String> = peer.keys.iter().map(Shown::to_string).collect();
    let private = if peer.private {
        "private events go to it"
    } else {
        "no private event goes to it"
    };
    let reached = match &peer.reached {
        None => String::new(),
        Some(Reached::Read) => "; its discovery document is read".to_owned(),
        Some(Reached::Unread(why)) => format!("; its discovery document is not read: {why}"),
    };
    format!(
        "peers.{}: {}, pinning {}, rotation {}, rate {}, most {}, {private}{reached}.",
        peer.name,
        peer.url,
        keys.join(", "),
        peer.rotation,
        peer.rate,
        peer.most
    )
}

/// `source` as a line.
fn source_line(source: &SourceShown) -> String {
    let pulls = match &source.pulls {
        Some(Pulls::Peer(peer)) => format!("pulls the feed of peers.{peer}"),
        Some(Pulls::Pinned(url)) => format!("pulls the feed of {url}"),
        None => "pulls no feed".to_owned(),
    };
    let pushed = if source.pushed {
        "takes pushes"
    } else {
        "takes no push"
    };
    let fixed = source
        .stale_after
        .map(|stale_after| {
            format!(
                " from its static log, taken up to {} after it was written",
                herald_core::Duration(stale_after).written()
            )
        })
        .unwrap_or_default();
    format!("sources.{}: {pulls}{fixed}, and {pushed}.", source.name)
}
