//! The record herald keeps of the last time each pull request left its own
//! queue: when, why and for which gesture, so a view's row holds it in
//! `queue.left` whatever the view reads (ADR-0087).

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{GestureKey, Leaving};
use crate::queue::Left;

/// The pull request a leaving is kept for: the base URL of the forge's
/// API, the repository's full path and the pull request's number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LeftKey {
    /// The base URL of the forge's API.
    pub api: String,
    /// The repository's full path.
    pub repository: String,
    /// The pull request's number.
    pub number: u64,
}

impl fmt::Display for LeftKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.api, self.repository, self.number)
    }
}

impl GestureKey {
    /// The pull request the gesture was made on.
    #[must_use]
    pub fn pull(&self) -> LeftKey {
        LeftKey {
            api: self.api.clone(),
            repository: self.repository.clone(),
            number: self.number,
        }
    }
}

/// When and why a pull request last left herald's own queue, and the
/// forge's id of the gesture it left for; never a title, a body or a
/// credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeftRecord {
    /// The pull request.
    pub key: LeftKey,
    /// The forge's id of the gesture it left for.
    pub gesture: u64,
    /// When herald recorded that it left, and why.
    pub left: Left,
}

impl LeftRecord {
    /// The leaving `leaving` of the pull request the gesture `gesture` was
    /// made on.
    #[must_use]
    pub fn of(gesture: &GestureKey, leaving: Leaving) -> Self {
        Self {
            key: gesture.pull(),
            gesture: gesture.gesture,
            left: Left {
                at: leaving.at,
                reason: leaving.left.dequeued(),
            },
        }
    }

    /// Whether it stands in place of `kept`, the leaving kept for its pull
    /// request: where none is, or `kept` is of another gesture or time and
    /// is no later.
    #[must_use]
    pub fn replaces(&self, kept: Option<&Self>) -> bool {
        kept.is_none_or(|kept| {
            kept != self && (kept.left.at, kept.gesture) <= (self.left.at, self.gesture)
        })
    }
}

#[cfg(test)]
mod tests;
