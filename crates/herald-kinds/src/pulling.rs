use std::fmt;

use herald_core::{Cursor, KindSpecs, Polled};
use herald_ports::{Boxed, DynHttp, DynVerifier};
use jiff::Timestamp;

/// One Ed25519 public key.
pub type PublicKey = [u8; 32];

/// Whose feed a pull reads: a peer of `[peers]`, or an instance whose keys
/// the source pins itself (ADR-0049).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peering<'a> {
    /// The peer of this name.
    Peer(&'a str),
    /// An instance the source pins itself.
    Pinned(Instance<'a>),
}

/// An instance another herald trusts: its name in its events' identities,
/// where it is served, its domain and the keys that stand for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instance<'a> {
    /// Its name, as its events' identities name it.
    pub name: &'a str,
    /// Where it is served: `https://`, its domain and the path its
    /// endpoints are under, with no trailing `/`.
    pub url: &'a str,
    /// Its domain, which its keys' ids name.
    pub domain: &'a str,
    /// The keys that stand for it.
    pub keys: &'a [PublicKey],
}

/// What a pull trusts and holds a peer's answers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trust {
    /// Where the peer pulled from is served.
    pub url: String,
    /// Its domain.
    pub domain: String,
    /// The keys that stand for it now.
    pub keys: Vec<PublicKey>,
    /// Every other instance an envelope it relays may come from: each peer
    /// of `[peers]` with the keys that stand for it.
    pub origins: Vec<Origin>,
    /// The most bytes of one of its answers read.
    pub most: usize,
    /// The most instances an envelope it passes on may have passed through.
    pub hops: u8,
    /// The most bytes a signature field or digest may hold.
    pub fields: usize,
    /// How many seconds a signature's creation may lie ahead of now, or its
    /// expiry behind.
    pub skew: i64,
    /// The most seconds a signature may live.
    pub longest: i64,
}

/// An instance an envelope may come from, with the keys that stand for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// Its name in `[peers]`, which identities it makes are taken under.
    pub name: String,
    /// Where it is served.
    pub url: String,
    /// Its domain.
    pub domain: String,
    /// The keys that stand for it.
    pub keys: Vec<PublicKey>,
}

/// What a pull reaches and reads with in one round.
#[derive(Clone, Copy)]
pub struct Pulling<'a> {
    /// What reaches the peer.
    pub http: &'a dyn DynHttp,
    /// What checks its signatures and proofs.
    pub verifier: &'a dyn DynVerifier,
    /// Every kind the config declares, which each event's kind must be one
    /// of.
    pub specs: &'a KindSpecs,
    /// The round's time.
    pub now: Timestamp,
}

impl fmt::Debug for Pulling<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pulling")
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

/// Why a pull found nothing to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unpulled {
    /// The peer could not be reached.
    Unreachable(String),
    /// Its answer could not be read, or does not hold.
    Unreadable(String),
    /// Its answer is signed by a key not trusted for it: the key's id.
    Stranger(String),
}

impl fmt::Display for Unpulled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(why) | Self::Unreadable(why) => f.write_str(why),
            Self::Stranger(keyid) => write!(
                f,
                "the feed's answer is signed by {keyid}, a key not trusted for the peer"
            ),
        }
    }
}

/// A source that pulls another herald's feed (ADR-0049).
pub trait Pull: fmt::Debug + Send + Sync {
    /// The feed's URL, whose origin is the only one its `Http` may reach.
    fn api(&self) -> &str;

    /// Whose feed it reads.
    fn peering(&self) -> Peering<'_>;

    /// How many identities its cursor remembers, newest first.
    fn remembered(&self) -> usize;

    /// What the feed lists since `cursor`, each answer and envelope checked
    /// against `trust` before it is read, or nothing where it says nothing
    /// changed.
    fn pulled<'a>(
        &'a self,
        pulling: Pulling<'a>,
        trust: &'a Trust,
        cursor: Option<&'a Cursor>,
    ) -> Boxed<'a, Result<Option<Polled>, Unpulled>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_s_round_shows_its_time_alone() {
        let specs = KindSpecs::default();
        let pulling = Pulling {
            http: &crate::reach::tests::Silent,
            verifier: &herald_testing::FakeVerifier,
            specs: &specs,
            now: Timestamp::UNIX_EPOCH,
        };
        assert_eq!(
            format!("{pulling:?}"),
            "Pulling { now: 1970-01-01T00:00:00Z, .. }"
        );
    }

    #[test]
    fn why_a_pull_read_nothing_reads_as_a_sentence() {
        assert_eq!(Unpulled::Unreachable("down".to_owned()).to_string(), "down");
        assert_eq!(
            Unpulled::Unreadable("garbled".to_owned()).to_string(),
            "garbled"
        );
        assert_eq!(
            Unpulled::Stranger("https://a.example/.well-known/herald#z6Mk".to_owned()).to_string(),
            "the feed's answer is signed by https://a.example/.well-known/herald#z6Mk, a key not trusted for the peer"
        );
    }
}
