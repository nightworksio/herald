use serde::{Deserialize, Serialize};

use crate::route::DestinationName;

/// One subscriber's opt-in to a project's announcements on a destination that
/// keeps no roles of its own: a Slack or Matrix user, or an ActivityPub
/// follower.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Subscription {
    /// The destination it is on.
    pub destination: DestinationName,
    /// The project it follows.
    pub project: String,
    /// The subscriber: a user id, or an ActivityPub follower's actor id.
    pub subscriber: String,
    /// Where an ActivityPub follower is delivered to: its server's shared
    /// inbox, or else its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox: Option<String>,
}
