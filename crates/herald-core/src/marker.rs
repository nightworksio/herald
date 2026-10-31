use std::fmt;

use crate::key::{Key, hex};

const DOMAIN: &[u8] = b"herald/marker/v1\0";

/// What an announcement carries on its destination to show herald posted it:
/// the MAC of its key under the installation's marker secret, in lowercase
/// hex (ADR-0011).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Marker(String);

impl Marker {
    /// The message whose MAC is `key`'s marker.
    #[must_use]
    pub fn message(key: &Key) -> Vec<u8> {
        [DOMAIN, key.as_bytes()].concat()
    }

    /// The marker spelled by `mac`.
    #[must_use]
    pub fn of(mac: &[u8]) -> Self {
        Self(hex(mac))
    }

    /// The marker as it is written on a post.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{Identity, RouteId};

    #[test]
    fn the_message_is_the_domain_and_the_key() {
        let key = Key::derive(&RouteId::new("r"), &Identity::new("i"));
        let message = Marker::message(&key);
        assert_eq!(&message[..17], b"herald/marker/v1\0");
        assert_eq!(&message[17..], key.as_bytes());
    }

    #[test]
    fn a_marker_spells_its_mac_in_lowercase_hex() {
        let marker = Marker::of(&[0x00, 0xab, 0xff]);
        assert_eq!(marker.as_str(), "00abff");
        assert_eq!(marker.to_string(), "00abff");
    }
}
