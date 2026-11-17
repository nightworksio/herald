use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::key::{Identity, RouteId, digest};
use crate::route::SourceName;

/// The longest herald holds an event back: a tag waiting for its Release,
/// or a group gathering its members.
pub const LONGEST_HOLD: jiff::SignedDuration = jiff::SignedDuration::from_hours(7 * 24);

/// What holds events back until they are due, and names the record they are
/// held in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "hold", rename_all = "kebab-case")]
pub enum Hold {
    /// A source's tag, waiting for the Release of its version (ADR-0032).
    Tag {
        /// The source.
        source: SourceName,
        /// The identity the tag and its Release share.
        identity: Identity,
    },
    /// A route's events that share a key, gathered into one announcement.
    Group {
        /// The route.
        route: RouteId,
        /// The key they share.
        key: String,
    },
}

impl Hold {
    /// The digest a store keeps the record under.
    #[must_use]
    pub fn digest(&self) -> String {
        match self {
            Self::Tag { source, identity } => {
                digest(&format!("tag\n{}\n{}", source.as_str(), identity.as_str()))
            }
            Self::Group { route, key } => digest(&format!("group\n{}\n{key}", route.as_str())),
        }
    }
}

/// One event held, with the source it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldEvent {
    /// The source it came from.
    pub source: SourceName,
    /// The forge id of the owner's member it is of, where the source is an
    /// owner's.
    pub member: Option<String>,
    /// The event.
    pub event: Event,
}

/// Events held back until they are due, and announced then unless what
/// holds them is let go first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// What holds them.
    pub hold: Hold,
    /// The events held, the first first.
    pub events: Vec<HeldEvent>,
    /// When the first was held.
    pub opened: Timestamp,
    /// When they are announced.
    pub due: Timestamp,
    /// When a held group stopped taking members to be announced, where it
    /// has; a member coming after is late.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<Timestamp>,
    /// Whether a group's announcement landed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub landed: bool,
}

impl Pending {
    /// Whether a group's record is kept no longer at `now`, its ledger
    /// entry kept for `kept`, or for ever where `None`: once that long
    /// passed after it closed, or after its window where it never closed.
    #[must_use]
    pub fn expired(&self, now: Timestamp, kept: Option<jiff::SignedDuration>) -> bool {
        kept.is_some_and(|kept| {
            self.closed
                .unwrap_or(self.due)
                .checked_add(kept)
                .is_ok_and(|until| until <= now)
        })
    }

    /// How many bytes its JSON takes.
    #[must_use]
    pub fn bytes(&self) -> usize {
        serde_json::to_vec(self).map_or(usize::MAX, |json| json.len())
    }

    /// Whether they are due at `now`.
    #[must_use]
    pub fn due_at(&self, now: Timestamp) -> bool {
        self.due <= now
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::tests::event;

    fn tag(source: &str, identity: &str) -> Hold {
        Hold::Tag {
            source: SourceName::new(source),
            identity: Identity::new(identity),
        }
    }

    #[test]
    fn each_hold_is_kept_under_its_own_digest() {
        let one = tag("cli", "github.com/1/v1");
        assert_eq!(one.digest(), tag("cli", "github.com/1/v1").digest());
        assert_ne!(one.digest(), tag("cli", "github.com/1/v2").digest());
        assert_ne!(one.digest(), tag("web", "github.com/1/v1").digest());
        assert_eq!(one.digest().len(), crate::SHA256_HEX_DIGITS);
    }

    fn group(route: &str, key: &str) -> Hold {
        Hold::Group {
            route: RouteId::new(route),
            key: key.to_owned(),
        }
    }

    #[test]
    fn each_group_is_kept_under_its_own_digest_apart_from_a_tag() {
        let one = group("train", "v1");
        assert_eq!(one.digest(), group("train", "v1").digest());
        assert_ne!(one.digest(), group("train", "v2").digest());
        assert_ne!(one.digest(), group("other", "v1").digest());
        assert_ne!(one.digest(), tag("train", "v1").digest());
        assert_eq!(
            serde_json::to_value(&one).unwrap(),
            serde_json::json!({"hold": "group", "route": "train", "key": "v1"})
        );
    }

    #[test]
    fn a_pending_record_reads_back_from_its_json_and_is_due_from_its_time() {
        let opened: Timestamp = "2026-11-15T10:00:00Z".parse().unwrap();
        let due: Timestamp = "2026-11-15T11:00:00Z".parse().unwrap();
        let pending = Pending {
            hold: tag("cli", "github.com/1/v1"),
            events: vec![HeldEvent {
                source: SourceName::new("cli"),
                member: Some("7".to_owned()),
                event: event(),
            }],
            opened,
            due,
            closed: None,
            landed: false,
        };
        let json = serde_json::to_value(&pending).unwrap();
        assert_eq!(
            json["hold"],
            serde_json::json!({"hold": "tag", "source": "cli", "identity": "github.com/1/v1"})
        );
        assert!(json.get("closed").is_none());
        assert!(json.get("landed").is_none());
        assert_eq!(serde_json::from_value::<Pending>(json).unwrap(), pending);
        assert!(!pending.due_at(opened));
        assert!(pending.due_at(due));
        assert!(pending.due_at(due + jiff::SignedDuration::from_secs(1)));
    }

    #[test]
    fn a_group_s_record_expires_its_ledger_s_time_after_it_closed_or_its_window() {
        let at = |text: &str| -> Timestamp { text.parse().unwrap() };
        let day = jiff::SignedDuration::from_hours(24);
        let mut pending = Pending {
            hold: group("train", "v1"),
            events: Vec::new(),
            opened: at("2026-11-15T10:00:00Z"),
            due: at("2026-11-15T11:00:00Z"),
            closed: None,
            landed: false,
        };
        assert!(!pending.expired(at("2026-11-16T10:59:59Z"), Some(day)));
        assert!(pending.expired(at("2026-11-16T11:00:00Z"), Some(day)));
        assert!(!pending.expired(at("2030-01-01T00:00:00Z"), None));
        pending.closed = Some(at("2026-11-15T12:00:00Z"));
        pending.landed = true;
        assert!(!pending.expired(at("2026-11-16T11:59:59Z"), Some(day)));
        assert!(pending.expired(at("2026-11-16T12:00:00Z"), Some(day)));
        assert!(!pending.expired(at("2026-11-16T12:00:00Z"), Some(jiff::SignedDuration::MAX)));
        let json = serde_json::to_value(&pending).unwrap();
        assert_eq!(json["closed"], "2026-11-15T12:00:00Z");
        assert_eq!(json["landed"], true);
        assert_eq!(serde_json::from_value::<Pending>(json).unwrap(), pending);
        assert_eq!(pending.bytes(), serde_json::to_vec(&pending).unwrap().len());
    }
}
