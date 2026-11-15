use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::key::{Identity, digest};
use crate::route::SourceName;

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
}

impl Hold {
    /// The digest a store keeps the record under.
    #[must_use]
    pub fn digest(&self) -> String {
        match self {
            Self::Tag { source, identity } => {
                digest(&format!("tag\n{}\n{}", source.as_str(), identity.as_str()))
            }
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
}

impl Pending {
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
        };
        let json = serde_json::to_value(&pending).unwrap();
        assert_eq!(
            json["hold"],
            serde_json::json!({"hold": "tag", "source": "cli", "identity": "github.com/1/v1"})
        );
        assert_eq!(serde_json::from_value::<Pending>(json).unwrap(), pending);
        assert!(!pending.due_at(opened));
        assert!(pending.due_at(due));
        assert!(pending.due_at(due + jiff::SignedDuration::from_secs(1)));
    }
}
