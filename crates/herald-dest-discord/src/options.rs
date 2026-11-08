use herald_core::{ReadBack, SecretRef};

use crate::interactions::{Interactions, Subscribe, snowflake};
use crate::look::{ArchiveAfter, Colours, Look};
use schemars::JsonSchema;
use serde::Deserialize;

/// How a Discord destination is reached, as `[destinations.*]` writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DestinationOptions {
    /// The secret holding a bot token. A bot crossposts, threads, pins and
    /// reads back its posts.
    pub token: Option<SecretRef>,
    /// The secret holding an incoming webhook's URL. A webhook only posts.
    pub webhook: Option<SecretRef>,
    /// Another base URL for Discord's API, such as a recorded-fixture server.
    pub api: Option<String>,
    /// How it takes commands, where it takes any.
    pub interactions: Option<Interactions>,
    /// How many pages of its own posts it reads back for their markers,
    /// from 1 to 20; 5 where unset.
    pub read_back: Option<ReadBack>,
    /// The colours of its embeds.
    pub colours: Option<Colours>,
    /// How long a quiet thread stays open: `1h`, `1d`, `3d` or `1w`, the
    /// week where unset.
    pub archive_after: Option<ArchiveAfter>,
}

impl DestinationOptions {
    /// How its posts look.
    #[must_use]
    pub fn look(&self) -> Look {
        Look::of(self.colours, self.archive_after)
    }
}

impl DestinationOptions {
    /// Whether the destination posts as a bot, or else through a webhook.
    ///
    /// # Errors
    ///
    /// When it names neither a token nor a webhook, or both.
    pub fn bot(&self) -> Result<bool, &'static str> {
        match (&self.token, &self.webhook) {
            (Some(_), None) => Ok(true),
            (None, Some(_)) => Ok(false),
            (None, None) => Err("a Discord destination needs a token or a webhook; give one"),
            (Some(_), Some(_)) => {
                Err("a Discord destination takes a token or a webhook, not both; keep one")
            }
        }
    }
}

/// What a route says about Discord alone, beside its crosspost, thread, pin
/// and mentions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteOptions {
    /// The channel's id. A bot needs one; a webhook posts where it was made.
    pub channel: Option<String>,
    /// Whether the channel is a forum, where each announcement is a post of
    /// its own.
    #[serde(default)]
    pub forum: bool,
    /// The opt-in role members subscribe to, where the route offers one.
    pub subscribe: Option<Subscribe>,
}

impl RouteOptions {
    /// Whether the options are sound.
    ///
    /// # Errors
    ///
    /// When `channel` is not written as Discord writes an id.
    pub fn check(&self) -> Result<(), String> {
        match &self.channel {
            Some(channel) if !snowflake(channel) => Err(format!(
                "\"{channel}\" is not a channel id; give the channel's id, 17 to 20 digits, from Copy Channel ID"
            )),
            _ => Ok(()),
        }
    }
}
