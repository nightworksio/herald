use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"herald/key/v1\0";
const HEX_DIGITS: usize = 64;

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

/// A text that is not a key: anything but 64 lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAKey;

impl fmt::Display for NotAKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a key is 64 lowercase hex digits")
    }
}

impl FromStr for Key {
    type Err = NotAKey;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let hex = text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !hex || text.len() != HEX_DIGITS {
            return Err(NotAKey);
        }
        let bytes: Vec<u8> = text
            .as_bytes()
            .chunks(2)
            .filter_map(|pair| std::str::from_utf8(pair).ok())
            .filter_map(|pair| u8::from_str_radix(pair, 16).ok())
            .collect();
        bytes.try_into().map(Self).map_err(|_| NotAKey)
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
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
    fn a_key_reads_back_from_its_hex() {
        let key = key("route", "identity");
        assert_eq!(key.to_string().parse::<Key>(), Ok(key));
    }

    #[test]
    fn anything_but_64_lowercase_hex_digits_is_not_a_key() {
        let hex = key("route", "identity").to_string();
        for text in [
            String::new(),
            hex[..63].to_owned(),
            format!("{hex}0"),
            format!("{hex}00"),
            hex.to_uppercase(),
            hex.replacen(&hex[..1], "g", 1),
        ] {
            assert_eq!(text.parse::<Key>(), Err(NotAKey), "{text}");
        }
        assert_eq!(NotAKey.to_string(), "a key is 64 lowercase hex digits");
    }

    #[test]
    fn a_key_serialises_as_its_hex() {
        let key = key("route", "identity");
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, format!("\"{key}\""));
        assert_eq!(serde_json::from_str::<Key>(&json).unwrap(), key);
        assert!(serde_json::from_str::<Key>("\"nope\"").is_err());
    }

    #[test]
    fn the_route_and_the_identity_keep_what_they_were_given() {
        assert_eq!(RouteId::new("route").as_str(), "route");
        assert_eq!(Identity::new("identity").as_str(), "identity");
    }
}
