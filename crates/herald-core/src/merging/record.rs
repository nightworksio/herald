//! The record herald keeps of each gesture that queued a pull request: the
//! head it first read with the gesture standing, and the step it took,
//! written before the forge is called, so herald takes a step for one
//! gesture at most once (ADR-0087).

use std::fmt;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// The gesture a record is of: its forge's API, its repository's full path,
/// the pull request's number, the forge's id of the label event or comment
/// it is, who made it and when the forge recorded it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GestureKey {
    /// The base URL of the forge's API.
    pub api: String,
    /// The repository's full path.
    pub repository: String,
    /// The pull request's number.
    pub number: u64,
    /// The forge's id of the label event or comment the gesture is: no two
    /// gestures share it, so a gesture made again is another record.
    pub gesture: u64,
    /// Who made the gesture, as the forge records its actor.
    pub by: String,
    /// When the forge recorded it.
    pub at: Timestamp,
}

impl fmt::Display for GestureKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}/{}/{}/{}/{}",
            self.api, self.repository, self.number, self.gesture, self.by, self.at
        )
    }
}

/// What became of a hand-off herald recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum HandedOff {
    /// Recorded before the call, with no answer recorded since: the call
    /// may or may not have reached the forge.
    Calling,
    /// The forge took the pull request into its queue, or reading it back
    /// found it there.
    Queued,
    /// The forge refused the call.
    Refused,
    /// The forge refused the call, and the pull request's head read back
    /// is no longer the one herald handed over.
    HeadMoved,
    /// The answer was lost and reading the pull request back did not find
    /// it in the forge's queue.
    Unsettled,
    /// Recorded, but the call was never sent: the run's request budget
    /// ended first, or the write credential could not be made ready for it.
    Unsent,
}

impl HandedOff {
    /// What it says, in plain words.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Calling => {
                "the call may or may not have reached the forge, so herald never hands it over again for this gesture"
            }
            Self::Queued => "the forge took it into its queue",
            Self::Refused => {
                "the forge refused to take it into its queue; a new gesture hands it over again"
            }
            Self::HeadMoved => {
                "the forge refused it, as its head is no longer the one handed over; a new gesture hands it over again"
            }
            Self::Unsettled => {
                "the forge's answer was lost and it was not found in the forge's queue, so herald never hands it over again for this gesture"
            }
            Self::Unsent => {
                "the call was not sent, as the run's request budget ended first or the write credential could not be made ready"
            }
        }
    }
}

/// The hand-off of one head to the forge's queue: the head, the base's
/// head read with it, the credential's name, when it was recorded, and
/// what became of it; never a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandOff {
    /// The head handed over, the one the forge was told to expect.
    pub head: String,
    /// The base branch's head when it was read before the step.
    pub base: String,
    /// The name of the secret or GitHub App the call was made with.
    pub credential: String,
    /// When it was recorded, before the call.
    pub at: Timestamp,
    /// What became of it.
    pub outcome: HandedOff,
}

/// The record of one gesture: the queue, the head herald first read with
/// it standing, and the hand-off it took, where it took one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GestureRecord {
    /// The gesture.
    pub key: GestureKey,
    /// The queue's name, as `[merge_queues.<name>]` names it.
    pub queue: String,
    /// The head herald first read with the gesture standing.
    pub seen: String,
    /// When herald first read it.
    pub seen_at: Timestamp,
    /// The hand-off, where herald recorded one.
    pub hand_off: Option<HandOff>,
}

impl GestureRecord {
    /// Whether a later run may hand the pull request over for this
    /// gesture: only where none was recorded, or the one recorded was
    /// never sent.
    #[must_use]
    pub fn hands_off(&self) -> bool {
        self.hand_off
            .as_ref()
            .is_none_or(|hand_off| hand_off.outcome == HandedOff::Unsent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(outcome: Option<HandedOff>) -> GestureRecord {
        let at: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();
        GestureRecord {
            key: GestureKey {
                api: "https://api.github.com".to_owned(),
                repository: "o/r".to_owned(),
                number: 7,
                gesture: 70,
                by: "w".to_owned(),
                at,
            },
            queue: "q".to_owned(),
            seen: "h1".to_owned(),
            seen_at: at,
            hand_off: outcome.map(|outcome| HandOff {
                head: "h1".to_owned(),
                base: "b1".to_owned(),
                credential: "HERALD_MERGE".to_owned(),
                at,
                outcome,
            }),
        }
    }

    #[test]
    fn a_gesture_is_handed_over_again_only_where_no_call_was_ever_sent() {
        assert!(record(None).hands_off());
        assert!(record(Some(HandedOff::Unsent)).hands_off());
        for outcome in [
            HandedOff::Calling,
            HandedOff::Queued,
            HandedOff::Refused,
            HandedOff::HeadMoved,
            HandedOff::Unsettled,
        ] {
            assert!(!record(Some(outcome)).hands_off(), "{outcome:?}");
            assert_ne!(outcome.said(), HandedOff::Unsent.said(), "{outcome:?}");
        }
        assert_eq!(
            record(None).key.to_string(),
            "https://api.github.com/o/r/7/70/w/2026-01-01T00:00:00Z"
        );
        let written = serde_json::to_string(&record(Some(HandedOff::HeadMoved))).unwrap();
        assert!(written.contains(r#""outcome":"head-moved""#), "{written}");
        let read: GestureRecord = serde_json::from_str(&written).unwrap();
        assert_eq!(read, record(Some(HandedOff::HeadMoved)));
    }
}
