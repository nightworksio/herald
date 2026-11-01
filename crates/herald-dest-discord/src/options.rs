use herald_core::SecretRef;
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
}
