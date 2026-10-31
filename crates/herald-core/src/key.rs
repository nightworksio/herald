use std::fmt;

use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"herald/key/v1\0";

/// A route's stable id, as the config names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RouteId(String);

impl RouteId {
    /// The route id `id`.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What makes an event that event, stable across reruns and polls.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Identity(String);

impl Identity {
    /// The identity `identity`.
    #[must_use]
    pub fn new(identity: impl Into<String>) -> Self {
        Self(identity.into())
    }

    /// The identity as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The never-twice key of one announcement, displayed in lowercase hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key([u8; 32]);

impl Key {
    /// The key of the announcement `route` makes of the event `identity`.
    #[must_use]
    pub fn derive(route: &RouteId, identity: &Identity) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(DOMAIN);
        for part in [route.as_str(), identity.as_str()] {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
        Self(hasher.finalize().into())
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(route: &str, identity: &str) -> Key {
        Key::derive(&RouteId::new(route), &Identity::new(identity))
    }

    #[test]
    fn a_key_is_the_sha256_of_the_domain_and_both_length_prefixed_parts() {
        assert_eq!(
            key("php-mutation-gate-releases", "github.com/1066469473/v0.1.0").to_string(),
            "da765393824b2f92d4e0df694048a62b994615c2ca027d5a69263e41b6828159"
        );
    }

    #[test]
    fn the_same_route_and_identity_give_the_same_key() {
        assert_eq!(key("route", "identity"), key("route", "identity"));
    }

    #[test]
    fn moving_bytes_between_the_route_and_the_identity_changes_the_key() {
        assert_ne!(key("ab", "c"), key("a", "bc"));
    }

    #[test]
    fn another_route_or_identity_gives_another_key() {
        assert_ne!(key("route", "identity"), key("other", "identity"));
        assert_ne!(key("route", "identity"), key("route", "other"));
    }

    #[test]
    fn the_route_and_the_identity_keep_what_they_were_given() {
        assert_eq!(RouteId::new("route").as_str(), "route");
        assert_eq!(Identity::new("identity").as_str(), "identity");
    }
}
