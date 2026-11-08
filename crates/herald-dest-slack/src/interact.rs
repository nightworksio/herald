//! Slack's commands: what a member asked, read from a slash command or a
//! button press, and herald's answers as the requests Slack takes
//! (ADR-0016).

use std::collections::BTreeMap;
use std::fmt;

use herald_core::{Answering, Asked, Command, DestinationName, Place, SecretRef};
use herald_ports::{Method, Request, Response, Secret, Text};
use serde_json::{Value, json};

use crate::interactions::{Interactions, Via};
use crate::read::quoted;
use crate::slack::{API, HOOKS};
use crate::web_api;

/// The action id of the button that subscribes to a project.
pub const SUBSCRIBE: &str = "herald_subscribe";
/// The word after a project that asks for the answer to be shown to the
/// channel.
const SHARE: &str = "share";
/// The most text a section block holds.
pub const SECTION: usize = 3000;
/// The most blocks a message holds.
const BLOCKS: usize = 50;

/// How a Slack destination takes and answers commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interacting {
    api: String,
    hooks: String,
    interactions: Interactions,
}

/// Where herald's answer to a command goes: the command's response URL,
/// open for thirty minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replying {
    /// The response URL, which carries its own token.
    pub url: Secret,
}

/// What a request Slack sent asks of herald.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    /// A command or a button press, to answer once it is worked.
    Asked {
        /// What was asked.
        asked: Box<Asked>,
        /// Where the answer goes.
        replying: Replying,
    },
    /// A command herald cannot read, answered at once with how to give one.
    Usage,
    /// Something herald does not answer, such as another app's button.
    Ignored,
}

/// Why a request could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// The body is neither a slash command nor a button press.
    NotACommand,
    /// It names no channel, no member or no response URL, or a response URL
    /// that is not Slack's.
    Unplaced,
}

impl fmt::Display for Unread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotACommand => "the body is neither a Slack command nor a button press",
            Self::Unplaced => {
                "the request names no channel, member or Slack response URL of its own"
            }
        })
    }
}

impl Interacting {
    /// How the destination with `interactions` takes commands, at `api` or
    /// Slack's own API, its response URLs on Slack's hooks or, where `api`
    /// is another base, on it.
    #[must_use]
    pub fn new(interactions: Interactions, api: Option<&str>) -> Self {
        let api = api.map(|api| api.trim_end_matches('/').to_owned());
        Self {
            hooks: api.clone().unwrap_or_else(|| HOOKS.to_owned()),
            api: api.unwrap_or_else(|| API.to_owned()),
            interactions,
        }
    }

    /// The base URL of the API it opens Socket Mode connections at.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    /// The base URL its answers are posted to.
    #[must_use]
    pub fn hooks(&self) -> &str {
        &self.hooks
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

    /// The secret holding the signing secret each request posted to herald
    /// is checked against.
    #[must_use]
    pub fn signing(&self) -> Option<&SecretRef> {
        self.interactions.signing.as_ref()
    }

    /// The secret holding the app-level token Socket Mode is opened with.
    #[must_use]
    pub fn app(&self) -> Option<&SecretRef> {
        self.interactions.app.as_ref()
    }

    /// How it answers, whatever carries the commands.
    #[must_use]
    pub fn answering(&self) -> Answering {
        self.interactions.answering()
    }

    /// What the request `body`, posted to `destination` as a form, asks: a
    /// slash command, or a button press carried in its `payload`.
    ///
    /// # Errors
    ///
    /// When it is neither, or names no channel, member or response URL of
    /// Slack's.
    pub fn received(&self, destination: &DestinationName, body: &[u8]) -> Result<Received, Unread> {
        let fields: BTreeMap<String, String> = url::form_urlencoded::parse(body)
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        if fields.get("ssl_check").map(String::as_str) == Some("1") {
            return Ok(Received::Ignored);
        }
        if let Some(payload) = fields.get("payload") {
            let payload: Value = serde_json::from_str(payload).map_err(|_| Unread::NotACommand)?;
            return self.pressed(destination, &payload);
        }
        self.command(destination, &fields)
    }

    /// What the slash command with `fields` asks, as Slack posts it or
    /// carries it over Socket Mode.
    ///
    /// # Errors
    ///
    /// As [`Interacting::received`].
    pub fn command(
        &self,
        destination: &DestinationName,
        fields: &BTreeMap<String, String>,
    ) -> Result<Received, Unread> {
        let field = |name: &str| fields.get(name).map(String::as_str);
        let given = field("command").ok_or(Unread::NotACommand)?;
        if given != self.interactions.command {
            return Ok(Received::Ignored);
        }
        let mut words = field("text").unwrap_or_default().split_whitespace();
        let Some(command) = words.next().and_then(Command::named) else {
            return Ok(Received::Usage);
        };
        let rest: Vec<&str> = words.collect();
        let (project, share) = match rest.as_slice() {
            [] => (None, false),
            [word] if word.eq_ignore_ascii_case(SHARE) => (None, true),
            [project] => (Some(*project), false),
            [project, word] if word.eq_ignore_ascii_case(SHARE) => (Some(*project), true),
            _ => return Ok(Received::Usage),
        };
        self.asked(
            destination,
            Placed {
                place: field("channel_id"),
                user: field("user_id"),
                url: field("response_url"),
            },
            command,
            project.map(ToOwned::to_owned),
            share,
            false,
        )
    }

    /// What the button press `payload`, posted to `destination` or carried
    /// over Socket Mode, asks.
    ///
    /// # Errors
    ///
    /// As [`Interacting::received`].
    pub fn pressed(
        &self,
        destination: &DestinationName,
        payload: &Value,
    ) -> Result<Received, Unread> {
        if payload.get("type").and_then(Value::as_str) != Some("block_actions") {
            return Ok(Received::Ignored);
        }
        let Some(project) = payload
            .get("actions")
            .and_then(Value::as_array)
            .and_then(|actions| {
                actions.iter().find(|action| {
                    action.get("action_id").and_then(Value::as_str) == Some(SUBSCRIBE)
                })
            })
            .and_then(|action| action.get("value"))
            .and_then(Value::as_str)
        else {
            return Ok(Received::Ignored);
        };
        let id = |key: &str| {
            payload
                .get(key)
                .and_then(|object| object.get("id"))
                .and_then(Value::as_str)
        };
        self.asked(
            destination,
            Placed {
                place: id("channel"),
                user: id("user"),
                url: payload.get("response_url").and_then(Value::as_str),
            },
            Command::Subscribe,
            Some(project.to_owned()),
            false,
            true,
        )
    }

    fn asked(
        &self,
        destination: &DestinationName,
        placed: Placed<'_>,
        command: Command,
        project: Option<String>,
        share: bool,
        toggles: bool,
    ) -> Result<Received, Unread> {
        let (Some(place), Some(user), Some(url)) =
            (given(placed.place), given(placed.user), placed.url)
        else {
            return Err(Unread::Unplaced);
        };
        if !url.starts_with(&format!("{}/", self.hooks)) {
            return Err(Unread::Unplaced);
        }
        Ok(Received::Asked {
            asked: Box::new(Asked {
                destination: destination.clone(),
                place: Place::new(place),
                user: user.to_owned(),
                command,
                project,
                share,
                toggles,
                member: None,
            }),
            replying: Replying {
                url: Secret::new("the command's response URL", url),
            },
        })
    }

    /// The response that says how to give a command, seen by the asker
    /// alone.
    #[must_use]
    pub fn usage(&self) -> String {
        let commands: Vec<&str> = self
            .interactions
            .commands
            .iter()
            .map(|command| command.name())
            .collect();
        json!({
            "response_type": "ephemeral",
            "text": format!(
                "Give {} {} with a project, and share to show the answer to the channel.",
                self.interactions.command,
                commands.join(" | ")
            ),
        })
        .to_string()
    }

    /// The request that answers the command `replying` names with `parts`,
    /// each within [`SECTION`], as one message seen by the asker alone
    /// unless `public`.
    #[must_use]
    pub fn answered(&self, replying: &Replying, parts: &[String], public: bool) -> Request {
        let blocks: Vec<Value> = parts
            .iter()
            .take(BLOCKS)
            .map(|part| json!({ "type": "section", "text": { "type": "mrkdwn", "text": part } }))
            .collect();
        let body = json!({
            "response_type": if public { "in_channel" } else { "ephemeral" },
            "replace_original": false,
            "text": parts.first().cloned().unwrap_or_default(),
            "blocks": blocks,
        });
        Request::new(Method::Post, Text::secret(replying.url.clone()))
            .header(
                "Content-Type",
                Text::plain("application/json; charset=utf-8"),
            )
            .body(body.to_string())
    }
}

impl Interacting {
    /// The app manifest that sets a Slack app up to give herald its
    /// commands: over Socket Mode, or posted to `url` where herald serves
    /// them at `/interactions/<destination>`.
    #[must_use]
    pub fn manifest(&self, destination: &str, url: Option<&str>) -> String {
        let endpoint =
            url.map(|url| format!("{}/interactions/{destination}", url.trim_end_matches('/')));
        let commands: Vec<&str> = self
            .interactions
            .commands
            .iter()
            .map(|command| command.name())
            .collect();
        let mut command = json!({
            "command": self.interactions.command,
            "description": "Ask herald about the projects it announces",
            "usage_hint": format!("{} [project] [share]", commands.join(" | ")),
            "should_escape": false,
        });
        let mut interactivity = json!({ "is_enabled": true });
        if let (Some(endpoint), Some(command), Some(interactivity)) = (
            &endpoint,
            command.as_object_mut(),
            interactivity.as_object_mut(),
        ) {
            command.insert("url".to_owned(), json!(endpoint));
            interactivity.insert("request_url".to_owned(), json!(endpoint));
        }
        json!({
            "display_information": { "name": "herald" },
            "features": {
                "bot_user": { "display_name": "herald", "always_online": false },
                "slash_commands": [command],
            },
            "oauth_config": {
                "scopes": {
                    "bot": web_api::bot_scopes(),
                },
            },
            "settings": {
                "interactivity": interactivity,
                "socket_mode_enabled": endpoint.is_none(),
                "org_deploy_enabled": false,
                "token_rotation_enabled": false,
            },
        })
        .to_string()
    }
}

/// `field`, where it was given and is not empty.
fn given(field: Option<&str>) -> Option<&str> {
    field.filter(|field| !field.is_empty())
}

#[derive(Clone, Copy)]
struct Placed<'a> {
    place: Option<&'a str>,
    user: Option<&'a str>,
    url: Option<&'a str>,
}

/// The block holding the button that subscribes to `project`, or
/// unsubscribes a member already subscribed.
#[must_use]
pub fn subscribe_block(project: &str) -> Value {
    json!({
        "type": "actions",
        "elements": [{
            "type": "button",
            "action_id": SUBSCRIBE,
            "value": project,
            "text": { "type": "plain_text", "text": "Subscribe" },
        }],
    })
}

/// Whether Slack took an answer, or else why not.
///
/// # Errors
///
/// What Slack said, where it refused or was unwell.
pub fn took(response: &Response) -> Result<(), String> {
    if response.succeeded() {
        Ok(())
    } else {
        Err(format!(
            "Slack answered {}: {}",
            response.status,
            quoted(&String::from_utf8_lossy(&response.body))
        )
        .trim_end_matches([' ', ':'])
        .to_owned())
    }
}

#[cfg(test)]
mod tests;
