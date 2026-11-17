//! Reading a peer's discovery document (ADR-0044): fetched within the
//! peer's body bound, its answer's signature checked against the keys
//! trusted for it before the document is read, so the keys it lists are
//! taken only as far as a trusted key vouches for them.

use std::fmt;

use herald_config::{Config, Federation};
use herald_core::{media, status};
use herald_peers::answer::{Answer, Bounds, Unanswered, checked};
use herald_peers::discovery;
use herald_peers::keys::{Key, KeySet, Unkeyed};
use herald_ports::{Algorithm, Http, Method, Request, SecretText, Verifier};
use jiff::Timestamp;

/// Where a peer's discovery document is, and what bounds its answer.
#[derive(Debug, Clone, Copy)]
pub struct Reading<'a> {
    /// The document's URL.
    pub url: &'a str,
    /// The peer's domain, which its keys' ids name.
    pub domain: &'a str,
    /// The most bytes of its body read.
    pub most: usize,
    /// What its signature is held to.
    pub bounds: Bounds,
}

/// A peer's discovery document read: the key set it lists, and the key
/// that signed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    /// The key set it lists.
    pub set: KeySet,
    /// The key that signed its answer.
    pub by: Key,
}

/// Why a peer's discovery document was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undiscovered {
    /// It could not be fetched: why.
    Unreached(String),
    /// It was answered with a status other than 200.
    Status(u16),
    /// Its answer's signature or digest does not hold.
    Unanswered(Unanswered),
    /// Its answer's signature is not the trusted key's.
    Forged,
    /// Its key set cannot be read.
    Unkeyed(Unkeyed),
}

impl fmt::Display for Undiscovered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreached(why) => write!(f, "the discovery document could not be fetched: {why}"),
            Self::Status(status) => write!(f, "the discovery document was answered {status}"),
            Self::Unanswered(why) => write!(f, "the discovery document's answer is refused: {why}"),
            Self::Forged => write!(f, "the discovery document's signature does not hold"),
            Self::Unkeyed(why) => why.fmt(f),
        }
    }
}

/// What `config` holds a peer's signed answer to, `now`: its signature
/// fields within `[federation] fields`, made and expiring within
/// `[server] inbound.replay_window` of now, and living no longer than any
/// herald signs an answer for.
#[must_use]
pub fn bounds(config: &Config, now: Timestamp) -> Bounds {
    Bounds {
        fields: config.federation.fields,
        now: now.as_second(),
        skew: config.server.inbound.replay_window.as_secs(),
        longest: Federation::SIGNED_FOR_ALLOWED.as_secs(),
    }
}

/// The discovery document `reading` names, fetched through `http`, its
/// answer checked through `verifier` against the `pinned` keys, or where
/// none are pinned yet, against the keys it lists itself.
///
/// # Errors
///
/// It cannot be fetched, is answered other than 200, its answer is not
/// signed by a trusted key over what herald demands, or it lists no key
/// set herald reads.
pub async fn discovered<H: Http, V: Verifier>(
    http: &H,
    verifier: &V,
    reading: Reading<'_>,
    pinned: Option<&[Key]>,
) -> Result<Discovered, Undiscovered> {
    let request = Request::new(Method::Get, SecretText::plain(reading.url))
        .accepting(media::JSON)
        .at_most(reading.most);
    let response = http.send(request).await.map_err(unreached)?;
    if response.status != status::OK {
        return Err(Undiscovered::Status(response.status));
    }
    let listed = pinned
        .is_none()
        .then(|| key_set(&response.body, reading.domain))
        .transpose()?;
    let trusted = pinned
        .or_else(|| listed.as_ref().map(|set| set.keys.as_slice()))
        .unwrap_or_default();
    let answer = Answer {
        method: Method::Get.as_str(),
        url: reading.url,
        status: response.status,
        fields: &response.headers,
        body: &response.body,
    };
    let (by, shown) = checked(&answer, reading.domain, trusted, reading.bounds)
        .map_err(Undiscovered::Unanswered)?;
    if !verifier
        .verify(Algorithm::Ed25519, by, &shown.base, &shown.signature)
        .await
    {
        return Err(Undiscovered::Forged);
    }
    let by = *by;
    let set = match listed {
        Some(set) => set,
        None => key_set(&response.body, reading.domain)?,
    };
    Ok(Discovered { set, by })
}

/// Why a fetch failed, as a document left unread.
fn unreached(error: impl fmt::Display) -> Undiscovered {
    Undiscovered::Unreached(error.to_string())
}

/// The key set the discovery document `body` of the instance on `domain`
/// lists.
fn key_set(body: &[u8], domain: &str) -> Result<KeySet, Undiscovered> {
    discovery::key_set(body, domain).map_err(Undiscovered::Unkeyed)
}
