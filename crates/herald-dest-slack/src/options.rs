use herald_core::{ReadBack, SecretRef, Symbols};

use crate::domain::Domain;

use crate::interactions::{Interactions, Subscribe};
use schemars::JsonSchema;
use serde::Deserialize;

/// How a Slack destination is reached, as `[destinations.*]` writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DestinationOptions {
    /// The secret holding a bot token (`xoxb-…`). A bot threads, pins and
    /// reads back its posts.
    pub token: Option<SecretRef>,
    /// The secret holding an incoming webhook's URL. A webhook only posts.
    pub webhook: Option<SecretRef>,
    /// Another base URL for Slack's API, such as a recorded-fixture server.
    pub api: Option<String>,
    /// The Slack the workspace lives on: `slack.com`, Slack's own and an
    /// Enterprise Grid's, where unset, or `slack-gov.com` for Slack's government cloud.
    pub domain: Option<Domain>,
    /// The workspace's id, such as `T0123456789`, where the token is an
    /// Enterprise Grid org-wide app's: each Web API call names it.
    pub team: Option<String>,
    /// How it takes commands, where it takes any.
    pub interactions: Option<Interactions>,
    /// How many pages of its own posts it reads back for their markers,
    /// from 1 to 20; 5 where unset.
    pub read_back: Option<ReadBack>,
    /// The characters its posts write where its markup has none: a task
    /// done and to do, a rule and a text list's bullet.
    pub symbols: Option<Symbols>,
}

impl DestinationOptions {
    /// Whether its workspace id, where it names one, is written as Slack
    /// writes one.
    ///
    /// # Errors
    ///
    /// When `team` is not a T followed by letters and digits.
    pub fn team(&self) -> Result<(), String> {
        match &self.team {
            Some(team) if !workspace_id(team) => Err(format!(
                "\"{team}\" is not a workspace id; give the workspace's id, a T and its letters and digits, such as \"T0123456789\""
            )),
            _ => Ok(()),
        }
    }

    /// Whether the destination posts as a bot, or else through a webhook.
    ///
    /// # Errors
    ///
    /// When it names neither a token nor a webhook, or both.
    pub fn bot(&self) -> Result<bool, &'static str> {
        match (&self.token, &self.webhook) {
            (Some(_), None) => Ok(true),
            (None, Some(_)) => Ok(false),
            (None, None) => Err("a Slack destination needs a token or a webhook; give one"),
            (Some(_), Some(_)) => {
                Err("a Slack destination takes a token or a webhook, not both; keep one")
            }
        }
    }
}

/// What a route says about Slack alone.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteOptions {
    /// The channel's id, such as `C0123456789`, which the bot has joined. A
    /// bot needs one; a webhook posts where it was made.
    pub channel: Option<String>,
    /// How the route offers subscribing, where it mentions its subscribers.
    pub subscribe: Option<Subscribe>,
}

impl RouteOptions {
    /// Whether the channel, where one is given, is written as a channel id.
    ///
    /// # Errors
    ///
    /// When it is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        match &self.channel {
            Some(channel) if !channel_id(channel) => Err(format!(
                "\"{channel}\" is not a channel id; give the channel's id, such as \"C0123456789\", from its details"
            )),
            _ => Ok(()),
        }
    }
}

fn channel_id(channel: &str) -> bool {
    channel.strip_prefix(['C', 'G', 'D']).is_some_and(|rest| {
        rest.len() >= 8
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    })
}

/// Whether `team` is written as a Slack workspace id: a T and at least two
/// capital letters or digits.
fn workspace_id(team: &str) -> bool {
    team.strip_prefix('T').is_some_and(|rest| {
        rest.len() >= 2
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    })
}
