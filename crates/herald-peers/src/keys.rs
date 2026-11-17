//! An instance's key set: the Ed25519 keys it signs with, newest first,
//! under a sequence number and a refresh hint, and the rotation statement
//! by which a key a peer pins hands over to a new set.

use std::fmt;

use serde_json::{Value, json};

use crate::multibase::{ED25519_LENGTH, ed25519_key, ed25519_public};
use crate::proof::{Shown, Unproven, shown};

/// The path of an instance's discovery document, which holds its key set.
pub const WELL_KNOWN: &str = "/.well-known/herald";
/// The type of each key in the set.
const MULTIKEY: &str = "Multikey";

/// One Ed25519 public key.
pub type Key = [u8; ED25519_LENGTH];

/// The Ed25519 keys an instance signs with, newest first: the first signs,
/// and every one verifies. An empty set withdraws every key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySet {
    /// Raised by one with each new set, so an older set never replaces a
    /// newer one.
    pub sequence: u64,
    /// How many seconds a reader may keep the set before reading it again.
    pub refresh: u64,
    /// The keys, newest first.
    pub keys: Vec<Key>,
}

/// The controller of every key the instance on `domain` holds: its
/// discovery document.
#[must_use]
pub fn controller(domain: &str) -> String {
    format!("https://{domain}{WELL_KNOWN}")
}

/// The id of `key` on `domain`: what a proof names as its verification
/// method and a signature as its `keyid`.
#[must_use]
pub fn method(domain: &str, key: &Key) -> String {
    format!("{}#{}", controller(domain), ed25519_public(key))
}

/// Why a key set cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unkeyed {
    /// The set is not of the shape herald writes: what is wrong.
    Shape(&'static str),
    /// A key's id or controller is not the instance's own.
    Foreign(String),
}

impl fmt::Display for Unkeyed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape(what) => write!(f, "the key set cannot be read: {what}"),
            Self::Foreign(id) => write!(f, "the key {id} is not the instance's own"),
        }
    }
}

impl KeySet {
    /// The key that signs: the newest.
    #[must_use]
    pub fn signing(&self) -> Option<&Key> {
        self.keys.first()
    }

    /// The key of the set that the id `method` names on `domain`.
    #[must_use]
    pub fn named(&self, domain: &str, method: &str) -> Option<&Key> {
        self.keys
            .iter()
            .find(|key| self::method(domain, key) == method)
    }

    /// The set as the discovery document of the instance on `domain`
    /// carries it.
    #[must_use]
    pub fn document(&self, domain: &str) -> Value {
        let keys: Vec<Value> = self
            .keys
            .iter()
            .map(|key| {
                json!({
                    "id": method(domain, key),
                    "type": MULTIKEY,
                    "controller": controller(domain),
                    "publicKeyMultibase": ed25519_public(key),
                })
            })
            .collect();
        json!({ "sequence": self.sequence, "refresh": self.refresh, "keys": keys })
    }

    /// The set the instance on `domain` publishes as `document`.
    ///
    /// # Errors
    ///
    /// The document is not of the shape [`KeySet::document`] writes, or a
    /// key's id or controller is not the instance's own.
    pub fn read(document: &Value, domain: &str) -> Result<Self, Unkeyed> {
        let number = |name: &str, wrong| {
            document
                .get(name)
                .and_then(Value::as_u64)
                .ok_or(Unkeyed::Shape(wrong))
        };
        let sequence = number("sequence", "it has no sequence")?;
        let refresh = number("refresh", "it has no refresh")?;
        let listed = document
            .get("keys")
            .and_then(Value::as_array)
            .ok_or(Unkeyed::Shape("it lists no keys"))?;
        let mut keys = Vec::with_capacity(listed.len());
        for entry in listed {
            let text = |name: &str| entry.get(name).and_then(Value::as_str);
            if text("type") != Some(MULTIKEY) {
                return Err(Unkeyed::Shape("a key is not a Multikey"));
            }
            let key = text("publicKeyMultibase")
                .and_then(ed25519_key)
                .ok_or(Unkeyed::Shape("a key is not an Ed25519 Multikey"))?;
            let id = text("id").unwrap_or_default();
            if id != method(domain, &key) || text("controller") != Some(controller(domain).as_str())
            {
                return Err(Unkeyed::Foreign(id.to_owned()));
            }
            keys.push(key);
        }
        Ok(Self {
            sequence,
            refresh,
            keys,
        })
    }
}

/// A rotation statement accepted so far: the new set, the pinned key that
/// signed it, and the proof that key's signature must check out against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rotation {
    /// The new set.
    pub set: KeySet,
    /// The pinned key that signed it.
    pub by: Key,
    /// Its proof.
    pub proof: Shown,
}

/// Why a rotation statement is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unrotated {
    /// Its proof cannot be read.
    Unproven(Unproven),
    /// Its set cannot be read.
    Unkeyed(Unkeyed),
    /// It is signed by a key the receiver does not pin.
    Stranger(String),
    /// Its sequence is not past the pinned set's.
    Stale {
        /// Its sequence.
        sequence: u64,
        /// The pinned set's.
        pinned: u64,
    },
}

impl fmt::Display for Unrotated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unproven(why) => why.fmt(f),
            Self::Unkeyed(why) => why.fmt(f),
            Self::Stranger(method) => write!(
                f,
                "the new key set is signed by {method}, which is not pinned"
            ),
            Self::Stale { sequence, pinned } => write!(
                f,
                "the new key set's sequence {sequence} is not past the pinned set's {pinned}"
            ),
        }
    }
}

/// The rotation statement `document` of the instance on `domain`, checked
/// against the `pinned` set: a key set, proved by a pinned key, with a
/// sequence past the pinned one. Its proof's signature is still to be
/// checked against [`Rotation::by`].
///
/// # Errors
///
/// Its proof or set cannot be read, a key that is not pinned signed it, or
/// its sequence is not past the pinned set's.
pub fn rotation(document: &Value, pinned: &KeySet, domain: &str) -> Result<Rotation, Unrotated> {
    let proof = shown(document).map_err(Unrotated::Unproven)?;
    let set = KeySet::read(document, domain).map_err(Unrotated::Unkeyed)?;
    let by = *pinned
        .named(domain, &proof.method)
        .ok_or_else(|| Unrotated::Stranger(proof.method.clone()))?;
    if set.sequence <= pinned.sequence {
        return Err(Unrotated::Stale {
            sequence: set.sequence,
            pinned: pinned.sequence,
        });
    }
    Ok(Rotation { set, by, proof })
}

#[cfg(test)]
mod tests;
