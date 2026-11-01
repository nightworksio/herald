use serde::{Deserialize, Serialize};

use crate::route::DestinationName;

/// One subscriber's opt-in to a project's announcements on a destination that
/// keeps no roles of its own: a Slack or Matrix user, or an ActivityPub
/// follower's inbox.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Subscription {
    /// The destination it is on.
    pub destination: DestinationName,
    /// The project it follows.
    pub project: String,
    /// The subscriber: a user id, or an inbox URL.
    pub subscriber: String,
}
