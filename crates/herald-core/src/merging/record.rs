//! The record herald keeps of each gesture that queued a pull request: the
//! head it first read with the gesture standing, and the step it took,
//! written before the forge is called, so herald takes a step for one
//! gesture at most once (ADR-0087).

use std::fmt;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// The gesture a record is of: its forge's API, its repository's full path,
/// the pull request's number and the forge's id of the label event or
/// comment it is, which the forge alone chooses.
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
}

impl fmt::Display for GestureKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}/{}/{}",
            self.api, self.repository, self.number, self.gesture
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
    /// Read right before the call, it was already in the forge's queue, so
    /// herald made no call: the gesture stands on it as on one herald
    /// handed over.
    FoundQueued,
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
            Self::FoundQueued => {
                "it was already in the forge's queue, so herald made no call; a cancel or a lost role takes it out as if herald had handed it over"
            }
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

/// The cancel that took a gesture back: the forge's id of the label event
/// or comment it is, who made it and when the forge recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelKey {
    /// The forge's id of the label event or comment the cancel is.
    pub id: u64,
    /// Who made it, as the forge records its actor.
    pub by: String,
    /// When the forge recorded it.
    pub at: Timestamp,
}

/// What became of taking a pull request out of the forge's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum TakenOut {
    /// Recorded before the call, with no answer recorded since: the call
    /// may or may not have reached the forge.
    Calling,
    /// The forge took the pull request out of its queue, or reading it
    /// back found it no longer there.
    Out,
    /// The forge refused the call, and reading it back found it still in
    /// the forge's queue.
    Refused,
    /// The answer was lost and reading the pull request back found it
    /// still in the forge's queue.
    Unsettled,
    /// Recorded, but the call was never sent: the run's request budget
    /// ended first, or the write credential could not be made ready for it.
    Unsent,
}

impl TakenOut {
    /// What it says, in plain words.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Calling => {
                "the call to take it out of the forge's queue may or may not have reached the forge, so herald never makes it again for this cancel"
            }
            Self::Out => "it is out of the forge's queue",
            Self::Refused => {
                "the forge refused to take it out of its queue, and it is still there; a new cancel asks again"
            }
            Self::Unsettled => {
                "the forge's answer was lost and it is still in the forge's queue, so herald never asks again for this cancel"
            }
            Self::Unsent => {
                "the call to take it out was not sent, as the run's request budget ended first or the write credential could not be made ready"
            }
        }
    }
}

/// Taking the pull request out of the forge's queue for one cancel, or
/// for the person who made the gesture no longer holding the queue's least
/// role: the cancel, the credential's name, when it was recorded, and what
/// became of it; never a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakeOut {
    /// The cancel it is for; none where it is for the role lost.
    pub cancel: Option<CancelKey>,
    /// The name of the secret or GitHub App the call was made with.
    pub credential: String,
    /// When it was recorded, before the call.
    pub at: Timestamp,
    /// What became of it.
    pub outcome: TakenOut,
}

/// The record of one gesture: the queue, the head herald first read with
/// it standing, the hand-off it took, where it took one, and each time it
/// took the pull request out of the forge's queue again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GestureRecord {
    /// The gesture.
    pub key: GestureKey,
    /// Who made it, as the forge recorded its actor when herald first read
    /// it.
    pub by: String,
    /// When the forge recorded it.
    pub at: Timestamp,
    /// The queue's name, as `[merge_queues.<name>]` names it.
    pub queue: String,
    /// The head herald first read with the gesture standing.
    pub seen: String,
    /// When herald first read it.
    pub seen_at: Timestamp,
    /// The hand-off, where herald recorded one.
    pub hand_off: Option<HandOff>,
    /// Each take-out herald recorded, one per cancel and one for the role
    /// lost.
    #[serde(default)]
    pub taken_out: Vec<TakeOut>,
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

    /// Whether the hand-off herald recorded may have put the pull request
    /// in the forge's queue: the forge took it, or its answer is not known.
    #[must_use]
    pub fn reached(&self) -> bool {
        self.hand_off.as_ref().is_some_and(|hand_off| {
            matches!(
                hand_off.outcome,
                HandedOff::Queued
                    | HandedOff::FoundQueued
                    | HandedOff::Calling
                    | HandedOff::Unsettled
            )
        })
    }

    /// The take-out recorded for `cancel`, or for the role lost where it
    /// is none, where one was.
    #[must_use]
    pub fn taken_out_for(&self, cancel: Option<&CancelKey>) -> Option<&TakeOut> {
        self.taken_out
            .iter()
            .find(|taken| taken.cancel.as_ref() == cancel)
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
            },
            by: "w".to_owned(),
            at,
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
            taken_out: Vec::new(),
        }
    }

    #[test]
    fn a_gesture_is_handed_over_again_only_where_no_call_was_ever_sent() {
        assert!(record(None).hands_off());
        assert!(record(Some(HandedOff::Unsent)).hands_off());
        for outcome in [
            HandedOff::Calling,
            HandedOff::Queued,
            HandedOff::FoundQueued,
            HandedOff::Refused,
            HandedOff::HeadMoved,
            HandedOff::Unsettled,
        ] {
            assert!(!record(Some(outcome)).hands_off(), "{outcome:?}");
            assert_ne!(outcome.said(), HandedOff::Unsent.said(), "{outcome:?}");
        }
        assert_eq!(
            record(None).key.to_string(),
            "https://api.github.com/o/r/7/70"
        );
        let written = serde_json::to_string(&record(Some(HandedOff::HeadMoved))).unwrap();
        assert!(written.contains(r#""outcome":"head-moved""#), "{written}");
        let read: GestureRecord = serde_json::from_str(&written).unwrap();
        assert_eq!(read, record(Some(HandedOff::HeadMoved)));
    }

    #[test]
    fn a_hand_off_may_have_put_it_in_the_forge_s_queue_unless_it_was_refused_or_never_sent() {
        assert!(!record(None).reached());
        for outcome in [
            HandedOff::Queued,
            HandedOff::FoundQueued,
            HandedOff::Calling,
            HandedOff::Unsettled,
        ] {
            assert!(record(Some(outcome)).reached(), "{outcome:?}");
        }
        for outcome in [HandedOff::Refused, HandedOff::HeadMoved, HandedOff::Unsent] {
            assert!(!record(Some(outcome)).reached(), "{outcome:?}");
        }
    }

    #[test]
    fn each_take_out_is_found_by_its_cancel_or_the_role_lost_alone_and_an_older_record_reads_none()
    {
        let at: Timestamp = "2026-01-02T00:00:00Z".parse().unwrap();
        let cancel = |id| CancelKey {
            id,
            by: "m".to_owned(),
            at,
        };
        let mut kept = record(Some(HandedOff::Queued));
        kept.taken_out.push(TakeOut {
            cancel: Some(cancel(80)),
            credential: "HERALD_MERGE".to_owned(),
            at,
            outcome: TakenOut::Refused,
        });
        assert_eq!(
            kept.taken_out_for(Some(&cancel(80)))
                .map(|taken| taken.outcome),
            Some(TakenOut::Refused)
        );
        assert!(kept.taken_out_for(Some(&cancel(81))).is_none());
        assert!(kept.taken_out_for(None).is_none());
        kept.taken_out.push(TakeOut {
            cancel: None,
            credential: "HERALD_MERGE".to_owned(),
            at,
            outcome: TakenOut::Out,
        });
        assert_eq!(
            kept.taken_out_for(None).map(|taken| taken.outcome),
            Some(TakenOut::Out)
        );
        let written = serde_json::to_string(&kept).unwrap();
        assert!(written.contains(r#""outcome":"refused""#), "{written}");
        assert_eq!(
            serde_json::from_str::<GestureRecord>(&written).unwrap(),
            kept
        );
        let older = written.replace(r#","taken_out":"#, r#","dropped":"#);
        let read: GestureRecord = serde_json::from_str(&older).unwrap();
        assert_eq!(read.taken_out, []);
        let said = [
            TakenOut::Calling,
            TakenOut::Out,
            TakenOut::Refused,
            TakenOut::Unsettled,
            TakenOut::Unsent,
        ]
        .map(TakenOut::said);
        let mut distinct = said.to_vec();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), said.len());
    }
}
