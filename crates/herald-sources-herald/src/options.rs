use std::num::NonZeroUsize;

use herald_core::kind_name;
use herald_kinds::{Fault, Instance, Peering, PublicKey};
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
}

impl HeraldOptions {
    /// How many pages one pull reads where `pages` is unset.
    pub const PAGES: usize = 5;
    /// The most pages one pull may read: four times the default.
    pub const PAGES_ALLOWED: usize = 4 * Self::PAGES;

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
        let most = pages.saturating_mul(PAGE_MOST);
        if file.backfill > most {
            return Err(Fault::at(
                "backfill",
                format!(
                    "backfill is at most {most}, the {pages} pages of {PAGE_MOST} one pull reads; lower it or raise pages"
                ),
            ));
        }
        if let Some(namespace) = file.namespaces.iter().find(|namespace| !granted(namespace)) {
            return Err(Fault::at(
                "namespaces",
                format!(
                    "\"{namespace}\" is not a namespace a peer may be granted; give the start of identities up to a '/', such as \"github.com/123456\", outside {NAMESPACE}/"
                ),
            ));
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
            _ => {
                return Err(Fault::here(
                    "name a peer of [peers] with its feed, as peer = \"<name>\" and feed = \"<feed>\", or a feed's url with the keys pinned for it, not both",
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
        })
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
