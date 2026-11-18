use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use herald_core::{Grant, kind_name};
use herald_kinds::{Bound, Fault, Instance, Peering, PublicKey};
use herald_peers::feed::{FeedAt, PAGE_MOST, feed_at, feed_of};
use herald_peers::multibase::ed25519_key;
use schemars::JsonSchema;
use serde::Deserialize;

/// The namespace every identity a peer gives is moved under unless a
/// source grants it, before the peer's name.
pub const NAMESPACE: &str = "herald";

/// Which envelopes a pull takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Verify {
    /// Only those the peer itself proved, which passed through no other
    /// instance.
    #[default]
    Origin,
    /// Also those the peer passes on from another peer of `[peers]`, each
    /// proved by that peer's key, through at most the peer's `hops`
    /// instances.
    Hop,
}

/// A `herald` source's options in `herald.toml`: whose feed it pulls, and
/// what it takes of it (ADR-0049).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeraldFile {
    /// The peer of `[peers]` whose feed it pulls, through the keys pinned
    /// for it; with `feed`, in place of `url`.
    #[schemars(example = &"herald-example-net")]
    pub peer: Option<String>,
    /// The name of the peer's feed, as its discovery document lists it.
    #[schemars(example = &"releases")]
    pub feed: Option<String>,
    /// The full URL of a feed of an instance that is no peer, whose keys
    /// `keys` pins; in place of `peer` and `feed`.
    #[schemars(example = "https://herald.example.net/herald/v1/feeds/releases")]
    pub url: Option<String>,
    /// The Ed25519 keys pinned for the instance `url` names, each a
    /// Multikey its operator gave out of band.
    #[serde(default)]
    pub keys: Vec<String>,
    /// How many of the newest envelopes the first pull announces; none
    /// where absent, and at most `pages` pages of 80.
    #[serde(default)]
    pub backfill: usize,
    /// How many pages one pull reads at most: 5 where absent, and at most
    /// 20.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub pages: Option<NonZeroUsize>,
    /// Which envelopes it takes: "origin" (where absent), those the peer
    /// proved itself, or "hop", also those it passes on from another peer.
    #[serde(default)]
    pub verify: Verify,
    /// The identities the peer may give as they are, each the start of
    /// identities up to a '/', such as "github.com/123456"; every other is
    /// taken as herald/<peer>/<identity>. None where absent.
    #[serde(default)]
    pub namespaces: Vec<String>,
    /// How many tombstones of identities it never took it keeps the proof
    /// of in all, so no older envelope of them is taken again, apart from
    /// those of identities it took, which it never forgets: 1000 where
    /// absent, and at most 4000. It forgets those taken first.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub tombstones: Option<NonZeroUsize>,
    /// How many tombstones of identities it never took it keeps of any one
    /// instance: 250 where absent, or `tombstones` where that is fewer, and
    /// at most `tombstones`.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub peer_tombstones: Option<NonZeroUsize>,
    /// How many tombstones of identities it took, which it never forgets,
    /// it keeps of any one instance before it takes no new identity of it:
    /// 1000 where absent, and at most 4000. A pull reads such an envelope
    /// again later; a tombstone of an identity it took is always taken.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub peer_share: Option<NonZeroUsize>,
    /// What each peer of `[peers]` may do with the source, by the peer's
    /// name, such as `{ upstream = { actions = ["push"] } }`; the peer's
    /// own `grants` must name the same. None where absent.
    #[serde(default)]
    pub peers: BTreeMap<String, Grant>,
}

/// A `herald` source, checked: whose feed it pulls, where, and what it
/// takes of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeraldOptions {
    /// The peer of `[peers]` it pulls from, where it names one.
    pub(crate) peer: Option<String>,
    /// The feed's name on the peer.
    pub(crate) feed: String,
    /// The instance's name in its events' identities: its peer's, or the
    /// domain of an instance the source pins itself, which no peer's name
    /// can be.
    pub(crate) name: String,
    /// Where the instance is served, once known.
    pub(crate) url: String,
    /// Its domain, once known.
    pub(crate) domain: String,
    /// The feed's URL, once known.
    pub(crate) api: String,
    /// The keys the source pins itself, where it names no peer.
    pub(crate) keys: Vec<PublicKey>,
    pub(crate) backfill: usize,
    pub(crate) pages: usize,
    pub(crate) verify: Verify,
    pub(crate) namespaces: Vec<String>,
    pub(crate) tombstones: usize,
    pub(crate) peer_tombstones: usize,
    pub(crate) peer_share: usize,
    /// What chose `pages`, `tombstones` and `peer_share`.
    pub(crate) bound: Bound,
    pub(crate) grants: BTreeMap<String, Grant>,
    /// Whether it pulls a feed, rather than only taking what its peers
    /// push to it.
    pub(crate) pulls: bool,
}

impl HeraldOptions {
    /// How many pages one pull reads where `pages` is unset.
    pub const PAGES: usize = 5;
    /// The most pages one pull may read: four times the default.
    pub const PAGES_ALLOWED: usize = 4 * Self::PAGES;
    /// How many tombstones a source keeps where `tombstones` is unset.
    pub const TOMBSTONES: usize = 1000;
    /// The most tombstones a source may keep: four times the default.
    pub const TOMBSTONES_ALLOWED: usize = 4 * Self::TOMBSTONES;
    /// How many tombstones of identities it never took a source keeps of
    /// one instance where `peer_tombstones` is unset: a quarter of the
    /// default `tombstones`, or `tombstones` where that is fewer.
    pub const PEER_TOMBSTONES: usize = Self::TOMBSTONES / 4;
    /// How many tombstones of identities it took a source keeps of one
    /// instance before it takes no new identity of it, where `peer_share`
    /// is unset.
    pub const PEER_SHARE: usize = 1000;
    /// The most `peer_share` may be: four times the default.
    pub const PEER_SHARE_ALLOWED: usize = 4 * Self::PEER_SHARE;

    /// The options `file` writes, where they are sound.
    ///
    /// # Errors
    ///
    /// It names neither or both of a peer and a URL, a peer without its
    /// feed, a URL without its keys, a key that is no Ed25519 Multikey or a
    /// setting past its bound.
    pub fn of(file: HeraldFile) -> Result<Self, Fault> {
        let pages = file.pages.map_or(Self::PAGES, NonZeroUsize::get);
        if pages > Self::PAGES_ALLOWED {
            return Err(Fault::at(
                "pages",
                format!("pages is at most {}; lower it", Self::PAGES_ALLOWED),
            ));
        }
        let (tombstones, peer_tombstones, peer_share) = Self::tombstoned(&file)?;
        let most = pages.saturating_mul(PAGE_MOST);
        if file.backfill > most {
            return Err(Fault::at(
                "backfill",
                format!(
                    "backfill is at most {most}, the {pages} pages of {PAGE_MOST} one pull reads; lower it or raise pages"
                ),
            ));
        }
        Self::granting(&file)?;
        let pulls = file.peer.is_some() || file.feed.is_some() || file.url.is_some();
        if !pulls && !file.peers.is_empty() {
            Self::pushed_only(&file)?;
        }
        let (peer, feed, at, keys) = match (file.peer, file.feed, file.url) {
            (Some(peer), Some(feed), None) => {
                named(&peer, &feed)?;
                (Some(peer), feed, FeedAt::default(), Vec::new())
            }
            (None, None, Some(url)) => {
                let at = feed_at(&url).ok_or_else(|| {
                    Fault::at(
                        "url",
                        format!("\"{url}\" is not a feed's URL; give https://, its domain and the path of the feed, such as \"https://herald.example/herald/v1/feeds/releases\""),
                    )
                })?;
                (None, String::new(), at, pinned(&file.keys)?)
            }
            (None, None, None) if !file.peers.is_empty() => {
                (None, String::new(), FeedAt::default(), Vec::new())
            }
            _ => {
                return Err(Fault::here(
                    "name a peer of [peers] with its feed, as peer = \"<name>\" and feed = \"<feed>\", or a feed's url with the keys pinned for it, not both; or for a source that only takes what peers push, neither, with the peers it grants",
                ));
            }
        };
        if peer.is_some() && !file.keys.is_empty() {
            return Err(Fault::at(
                "keys",
                "a peer's keys are pinned in [peers]; leave keys out, or name a url in place of the peer",
            ));
        }
        Ok(Self {
            name: peer.clone().unwrap_or_else(|| at.domain.clone()),
            peer,
            feed,
            url: at.instance,
            domain: at.domain,
            api: at.feed,
            keys,
            backfill: file.backfill,
            pages,
            verify: file.verify,
            namespaces: file.namespaces,
            tombstones,
            peer_tombstones,
            peer_share,
            bound: if file.pages.is_some() || file.tombstones.is_some() || file.peer_share.is_some()
            {
                Bound::Named
            } else {
                Bound::Default
            },
            grants: file.peers,
            pulls,
        })
    }

    /// Whether what `file` grants each peer is sound: a peer's name, at
    /// least one action, and namespaces a peer may be granted.
    fn granting(file: &HeraldFile) -> Result<(), Fault> {
        for (peer, grant) in &file.peers {
            let at = format!("peers.{peer}");
            if !kind_name(peer) {
                return Err(Fault::at(
                    "peers",
                    format!("\"{peer}\" is not a peer's name; name a table of [peers]"),
                ));
            }
            if grant.actions.is_empty() {
                return Err(Fault::at(
                    "peers",
                    format!("{at} grants no action; grant one, such as actions = [\"push\"]"),
                ));
            }
            if let Some(namespace) = grant
                .namespaces
                .iter()
                .find(|namespace| !granted(namespace))
            {
                return Err(Fault::at(
                    "peers",
                    format!(
                        "{at}: \"{namespace}\" is not a namespace a peer may be granted; give the start of identities up to a '/', such as \"github.com/123456\", outside {NAMESPACE}/"
                    ),
                ));
            }
        }
        if let Some(namespace) = file.namespaces.iter().find(|namespace| !granted(namespace)) {
            return Err(Fault::at(
                "namespaces",
                format!(
                    "\"{namespace}\" is not a namespace a peer may be granted; give the start of identities up to a '/', such as \"github.com/123456\", outside {NAMESPACE}/"
                ),
            ));
        }
        Ok(())
    }

    /// Whether `file`, which pulls no feed, names nothing only a pull
    /// reads.
    fn pushed_only(file: &HeraldFile) -> Result<(), Fault> {
        match (file.keys.is_empty(), file.namespaces.is_empty()) {
            (false, _) => Err(Fault::at(
                "keys",
                "a source that only takes what peers push pins no keys; each peer's keys are pinned in [peers]",
            )),
            (_, false) => Err(Fault::at(
                "namespaces",
                "a source that only takes what peers push grants namespaces to each peer, in peers.<name>.namespaces",
            )),
            (true, true) => Ok(()),
        }
    }

    /// How many tombstones `file` keeps, in all and of one instance, where
    /// both are within their bounds.
    fn tombstoned(file: &HeraldFile) -> Result<(usize, usize, usize), Fault> {
        let tombstones = file.tombstones.map_or(Self::TOMBSTONES, NonZeroUsize::get);
        if tombstones > Self::TOMBSTONES_ALLOWED {
            return Err(Fault::at(
                "tombstones",
                format!(
                    "tombstones is at most {}; lower it",
                    Self::TOMBSTONES_ALLOWED
                ),
            ));
        }
        let peer_tombstones = file
            .peer_tombstones
            .map_or(Self::PEER_TOMBSTONES.min(tombstones), NonZeroUsize::get);
        if peer_tombstones > tombstones {
            return Err(Fault::at(
                "peer_tombstones",
                format!(
                    "peer_tombstones is at most tombstones, {tombstones}; lower it or raise tombstones"
                ),
            ));
        }
        let peer_share = file.peer_share.map_or(Self::PEER_SHARE, NonZeroUsize::get);
        if peer_share > Self::PEER_SHARE_ALLOWED {
            return Err(Fault::at(
                "peer_share",
                format!(
                    "peer_share is at most {}; lower it",
                    Self::PEER_SHARE_ALLOWED
                ),
            ));
        }
        Ok((tombstones, peer_tombstones, peer_share))
    }

    /// Whose feed the source reads.
    #[must_use]
    pub(crate) fn reads(&self) -> Peering<'_> {
        match &self.peer {
            Some(peer) => Peering::Peer(peer),
            None => Peering::Pinned(Instance {
                name: &self.name,
                url: &self.url,
                domain: &self.domain,
                keys: &self.keys,
            }),
        }
    }

    /// The options of a source pulling from its peer, served at `url`.
    #[must_use]
    pub(crate) fn served_at(&self, url: &str) -> Self {
        Self {
            api: feed_of(url, &self.feed),
            ..self.clone()
        }
    }
}

/// Whether the peer `peer` and its feed `feed` are named as a peer and a
/// feed are.
fn named(peer: &str, feed: &str) -> Result<(), Fault> {
    if !kind_name(peer) {
        return Err(Fault::at(
            "peer",
            format!("\"{peer}\" is not a peer's name; name a table of [peers]"),
        ));
    }
    if feed.is_empty() {
        return Err(Fault::at(
            "feed",
            "name the peer's feed, as its discovery document lists it",
        ));
    }
    Ok(())
}

/// The keys `written` pins, each once and at least one.
fn pinned(written: &[String]) -> Result<Vec<PublicKey>, Fault> {
    let mut keys = Vec::with_capacity(written.len());
    for key in written {
        match ed25519_key(key) {
            Some(read) if !keys.contains(&read) => keys.push(read),
            Some(_) => {
                return Err(Fault::at(
                    "keys",
                    format!("\"{key}\" is pinned twice; pin each key once"),
                ));
            }
            None => {
                return Err(Fault::at(
                    "keys",
                    format!(
                        "\"{key}\" is not an Ed25519 Multikey; pin the keys the instance's operator gave, each such as \"z6Mk…\""
                    ),
                ));
            }
        }
    }
    if keys.is_empty() {
        return Err(Fault::at("keys", "pin at least one of the instance's keys"));
    }
    Ok(keys)
}

/// Whether `namespace` is one a peer may be granted: the start of
/// identities, not ending in '/', outside the namespace peers' own
/// identities are moved under.
fn granted(namespace: &str) -> bool {
    !namespace.is_empty()
        && !namespace.ends_with('/')
        && namespace != NAMESPACE
        && !namespace.starts_with(&format!("{NAMESPACE}/"))
}
