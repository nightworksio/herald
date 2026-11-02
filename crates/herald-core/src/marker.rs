use std::fmt;

use crate::key::{Key, RouteId, hex};

const DOMAIN: &[u8] = b"herald/marker/v1\0";
const ROUTE_DOMAIN: &[u8] = b"herald/route/v1\0";

/// What a post carries on its destination to show herald made it: a MAC
/// under the installation's marker key, in lowercase hex (ADR-0011).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Marker(String);

impl Marker {
    /// The message whose MAC is `key`'s marker.
    #[must_use]
    pub fn message(key: &Key) -> Vec<u8> {
        [DOMAIN, key.as_bytes()].concat()
    }

    /// The message whose MAC is `route`'s marker.
    #[must_use]
    pub fn route_message(route: &RouteId) -> Vec<u8> {
        [ROUTE_DOMAIN, route.as_str().as_bytes()].concat()
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

/// The markers an announcement's post carries: its route's, and its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Marks {
    /// The MAC of the route id, the same on every post of the route.
    pub route: Marker,
    /// The MAC of the announcement's key.
    pub announcement: Marker,
}

/// What a link's fragment starts with where it carries marks.
const FRAGMENT: &str = "#herald-";

impl Marks {
    /// `link` carrying these marks as its fragment,
    /// `#herald-<route>.<announcement>`, in place of any it had.
    #[must_use]
    pub fn on(&self, link: &str) -> String {
        let page = link.split('#').next().unwrap_or(link);
        format!("{page}{FRAGMENT}{}.{}", self.route, self.announcement)
    }

    /// The route and announcement markers `link` carries as its fragment,
    /// as written, where it carries them.
    #[must_use]
    pub fn carried(link: &str) -> Option<(&str, &str)> {
        link.rsplit_once(FRAGMENT)
            .and_then(|(_, fragment)| fragment.split_once('.'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Identity;

    #[test]
    fn the_message_is_the_domain_and_the_key() {
        let key = Key::derive(&RouteId::new("r"), &Identity::new("i"));
        let message = Marker::message(&key);
        assert_eq!(&message[..17], b"herald/marker/v1\0");
        assert_eq!(&message[17..], key.as_bytes());
    }

    #[test]
    fn the_route_message_is_its_domain_and_the_route_id() {
        let message = Marker::route_message(&RouteId::new("releases"));
        assert_eq!(message, b"herald/route/v1\0releases");
    }

    #[test]
    fn a_link_carries_its_marks_as_its_fragment_in_place_of_any_it_had() {
        let marks = Marks {
            route: Marker::of(&[0x0a]),
            announcement: Marker::of(&[0x0b]),
        };
        for link in ["https://a.example/v1", "https://a.example/v1#notes"] {
            let marked = marks.on(link);
            assert_eq!(marked, "https://a.example/v1#herald-0a.0b");
            assert_eq!(Marks::carried(&marked), Some(("0a", "0b")));
        }
        assert_eq!(Marks::carried("https://a.example/v1#notes"), None);
        assert_eq!(Marks::carried("https://a.example/v1#herald-0a"), None);
    }

    #[test]
    fn a_marker_spells_its_mac_in_lowercase_hex() {
        let marker = Marker::of(&[0x00, 0xab, 0xff]);
        assert_eq!(marker.as_str(), "00abff");
        assert_eq!(marker.to_string(), "00abff");
    }
}
