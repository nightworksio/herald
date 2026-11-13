//! Discord's interactions: what a member asked, read from the interaction
//! Discord sends, and herald's answers, roles and commands as the requests
//! Discord takes (ADR-0016).

use herald_core::Answering;
use herald_ports::{Method, Request, Response, Secret, SecretText};
use serde_json::Value;

use crate::discord::{API, DISCORD};
use crate::interactions::{Interactions, Via};
use crate::read::{error_text, waited};
use crate::request::{bare, json};

mod answers;
mod commands;
mod received;
mod roles;

pub use answers::{MESSAGE, choices, deferred, pong};
pub use received::{Received, UnreadInteraction};
pub use roles::{listed, made};

/// An option holding a subcommand.
const SUB_COMMAND: u64 = 1;
/// The longest a button's custom id may be.
const CUSTOM_ID: usize = 100;
/// The custom id a subscribe button starts with, before the project.
const SUBSCRIBE: &str = "herald:subscribe:";
/// The option naming the project.
const PROJECT: &str = "project";
/// The option asking for the answer to be shown to the channel.
const SHARE: &str = "share";

/// How a Discord destination takes and answers commands, with the bot token
/// it gives roles with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interacting {
    api: String,
    interactions: Interactions,
    bot: Option<Secret>,
}

/// Where herald's answer to an interaction goes: the interaction's own
/// webhook, open for fifteen minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replying {
    /// The interaction's id.
    pub id: String,
    /// The interaction's token.
    pub token: Secret,
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

impl Interacting {
    /// How the destination with `interactions` takes commands, giving roles
    /// as the bot whose token is `bot`, at `api` or Discord's own API.
    #[must_use]
    pub fn new(interactions: Interactions, bot: Option<Secret>, api: Option<&str>) -> Self {
        Self {
            api: api.unwrap_or(API).trim_end_matches('/').to_owned(),
            interactions,
            bot,
        }
    }

    /// The base URL of the API it reaches.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    /// How it carries commands.
    #[must_use]
    pub fn via(&self) -> Via {
        self.interactions.via
    }

    /// The domains its sessions may connect under.
    #[must_use]
    pub fn session_domains(&self) -> Vec<&str> {
        self.interactions.session_domains()
    }

    /// The operating system its gateway session identifies as, where the
    /// config names one.
    #[must_use]
    pub fn os(&self) -> Option<&str> {
        self.interactions.os.as_deref()
    }

    /// The application's public key, in hex, which each interaction posted
    /// to herald is checked against.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        self.interactions.key.as_deref()
    }

    /// How it answers, whatever carries the commands.
    #[must_use]
    pub fn answering(&self) -> Answering {
        self.interactions.answering()
    }

    /// The bot token it joins the gateway with.
    #[must_use]
    pub fn bot(&self) -> Option<&Secret> {
        self.bot.as_ref()
    }

    fn bare(&self, method: Method, path: &str) -> Option<Request> {
        let url = SecretText::plain(format!("{}{path}", self.api));
        Some(bare(method, url, Some(self.bot.as_ref()?)))
    }

    fn json(&self, method: Method, path: &str, body: &Value) -> Option<Request> {
        let url = SecretText::plain(format!("{}{path}", self.api));
        Some(json(
            method,
            url,
            Some(self.bot.as_ref()?),
            body.to_string(),
        ))
    }
}

/// The custom id of the button that subscribes to `project`, where it fits
/// Discord's limit.
#[must_use]
pub fn subscribe_button(project: &str) -> Option<String> {
    let id = format!("{SUBSCRIBE}{project}");
    (id.len() <= CUSTOM_ID).then_some(id)
}

/// Whether Discord took a request about roles, commands or an answer, or
/// else why not.
///
/// # Errors
///
/// What Discord said, where it refused or was unwell.
pub fn accepted(response: &Response) -> Result<(), String> {
    let body = crate::read::json(response);
    response.accepted(waited(&body), DISCORD, || error_text(&body))
}

#[cfg(test)]
mod tests;
