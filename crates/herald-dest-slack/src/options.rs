use herald_core::{PinLookups, ReadBack, SecretRef, Symbols, WaitOptions, bot_or_webhook};

use crate::domain::Domain;
use crate::slack::SLACK;

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
    /// How many of its own pins it looks up, one request each, as it finds
    /// a route's earlier pins, from 1 to 100; 25 where unset.
    pub pin_lookups: Option<PinLookups>,
    /// The characters its posts write where its markup has none: a task
    /// done and to do, a rule and a text list's bullet.
    pub symbols: Option<Symbols>,
    /// How long it waits before sending again where Slack answers that it
    /// is unwell or limited and names no wait.
    pub waits: Option<WaitOptions>,
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
    pub fn bot(&self) -> Result<bool, String> {
        bot_or_webhook(SLACK, self.token.as_ref(), self.webhook.as_ref())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn team(team: &str) -> Result<(), String> {
        serde_json::from_value::<DestinationOptions>(serde_json::json!({ "team": team }))
            .unwrap()
            .team()
    }

    #[test]
    fn a_workspace_is_named_by_its_id() {
        for written in ["T0123456789", "T01", "TAB"] {
            assert_eq!(team(written), Ok(()), "{written}");
        }
        let unnamed: DestinationOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(unnamed.team(), Ok(()));
        for written in ["T", "T0", "t0123456789", "T01x", "X0123456789", "T01-2"] {
            assert_eq!(
                team(written),
                Err(format!(
                    "\"{written}\" is not a workspace id; give the workspace's id, a T and its letters and digits, such as \"T0123456789\""
                )),
                "{written}"
            );
        }
    }
}
