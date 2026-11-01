//! The sample every destination's fixtures announce.

use herald_core::{
    Announcement, Delivery, Identity, Key, Marker, Mentions, RoleId, RouteId, Text, Thread,
};

/// The route every destination's fixtures announce through.
pub const ROUTE: &str = "php-mutation-gate-releases";

/// The marker the sample announcement carries.
#[must_use]
pub fn marker() -> Marker {
    Marker::of(&[0xa5; 32])
}

/// The announcement every destination's fixtures send: php-mutation-gate
/// v0.1.0, in two parts, threaded and linked.
#[must_use]
pub fn announcement() -> Announcement {
    Announcement {
        key: Key::derive(
            &RouteId::new(ROUTE),
            &Identity::new("github.com/1066469473/v0.1.0"),
        ),
        marker: marker(),
        route: RouteId::new(ROUTE),
        title: "📦 php-mutation-gate v0.1.0".to_owned(),
        parts: vec![
            Text {
                body: "## Added".to_owned(),
                html: None,
            },
            Text {
                body: "- the gate".to_owned(),
                html: None,
            },
        ],
        thread: Some("php-mutation-gate v0.1.0".to_owned()),
        link: Some(
            "https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0".to_owned(),
        ),
        prerelease: false,
        occurred: jiff::Timestamp::constant(1_793_448_000, 0),
    }
}

/// Every step of the sample announcement: crossposted, threaded, pinned, and
/// pinging `role`.
#[must_use]
pub fn delivery(role: &str) -> Delivery {
    Delivery {
        crosspost: true,
        thread: Some(Thread {
            name: String::new(),
            archive_after: None,
            lock_after: None,
        }),
        pin: true,
        mentions: Mentions {
            roles: vec![RoleId::new(role)],
            subscribers: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_happened_at_noon_on_the_last_day_of_october_2026() {
        assert_eq!(announcement().occurred.to_string(), "2026-10-31T12:00:00Z");
        assert_eq!(announcement().marker, marker());
        assert_eq!(delivery("1").mentions.roles, [RoleId::new("1")]);
    }
}
