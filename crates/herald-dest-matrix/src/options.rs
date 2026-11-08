use herald_core::{ReadBack, SecretRef};

use crate::interactions::Interactions;
use schemars::JsonSchema;
use serde::Deserialize;

/// How a Matrix destination is reached, as `[destinations.*]` writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DestinationOptions {
    /// The homeserver's base URL, such as `https://matrix.org`.
    pub homeserver: String,
    /// The secret holding the bot account's access token.
    pub token: SecretRef,
    /// How it takes commands, where it takes any.
    pub interactions: Option<Interactions>,
    /// How many pages of its own posts it reads back for their markers,
    /// from 1 to 20; 5 where unset.
    pub read_back: Option<ReadBack>,
}

/// What a route says about Matrix alone.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteOptions {
    /// The room's id, such as `!abcdefg:matrix.org`, which the bot has
    /// joined.
    pub room: String,
}

impl RouteOptions {
    /// Whether the room is written as a room id.
    ///
    /// # Errors
    ///
    /// When it is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        let sound = self
            .room
            .strip_prefix('!')
            .and_then(|rest| rest.split_once(':'))
            .is_some_and(|(local, server)| !local.is_empty() && !server.is_empty());
        if sound {
            Ok(())
        } else {
            Err(format!(
                "\"{}\" is not a room id; give the room's id, such as \"!abcdefg:matrix.org\", from its settings",
                self.room
            ))
        }
    }
}
