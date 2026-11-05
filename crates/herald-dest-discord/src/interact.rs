//! Discord's interactions: what a member asked, read from the interaction
//! Discord sends, and herald's answers, roles and commands as the requests
//! Discord takes (ADR-0016).

use std::fmt;

use herald_core::{Answering, Asked, Command, DestinationName, Member};
use herald_ports::{Method, Request, Response, Secret, Text};
use serde_json::{Value, json};

use crate::discord::API;
use crate::interactions::{Interactions, Via, snowflake};

/// Discord's ping, which an endpoint answers with a pong.
const PING: u64 = 1;
/// A slash command.
const APPLICATION_COMMAND: u64 = 2;
/// A press of a message component, such as a button.
const MESSAGE_COMPONENT: u64 = 3;
/// A member typing into an option that completes.
const AUTOCOMPLETE: u64 = 4;
/// The answer to a ping.
const PONG: u64 = 1;
/// An answer that says herald is working, to be edited once it has worked.
const DEFERRED: u64 = 5;
/// An answer listing the completions of what a member typed.
const CHOICES: u64 = 8;
/// The flag of a message the asker alone sees.
const EPHEMERAL: u64 = 64;
/// An option holding a subcommand.
const SUB_COMMAND: u64 = 1;
/// An option holding text.
const STRING: u64 = 3;
/// An option holding true or false.
const BOOLEAN: u64 = 5;
/// The longest a button's custom id may be.
const CUSTOM_ID: usize = 100;
/// The custom id a subscribe button starts with, before the project.
const SUBSCRIBE: &str = "herald:subscribe:";
/// The option naming the project.
const PROJECT: &str = "project";
/// The option asking for the answer to be shown to the channel.
const SHARE: &str = "share";
/// The longest message Discord takes, in characters.
pub const MESSAGE: usize = 2000;

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

/// What an interaction Discord sent asks of herald.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    /// Discord checking the endpoint answers.
    Ping,
    /// A command or a button press, to answer once it is worked.
    Asked {
        /// What was asked.
        asked: Asked,
        /// Where the answer goes.
        replying: Replying,
    },
    /// A member typing a project's name, to be answered at once with the
    /// projects it completes to.
    Completing {
        /// The channel it is typed in.
        place: String,
        /// What has been typed.
        typed: String,
        /// Where the completions go where no endpoint answers them.
        replying: Replying,
    },
    /// Something herald does not answer, such as another application's
    /// button.
    Ignored,
}

/// Why an interaction could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// The body is not an interaction.
    NotAnInteraction,
    /// It names no channel or no member.
    Unplaced,
}

impl fmt::Display for Unread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAnInteraction => "the body is not a Discord interaction",
            Self::Unplaced => "the interaction names no channel or no member",
        })
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn options(value: &Value) -> &[Value] {
    value
        .get("options")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn option<'a>(options: &'a [Value], name: &str) -> Option<&'a Value> {
    options
        .iter()
        .find(|option| text(option, "name") == Some(name))
        .and_then(|option| option.get("value"))
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

    /// What the interaction `body`, sent to `destination`, asks.
    ///
    /// # Errors
    ///
    /// When it is not an interaction, or names no channel or no member.
    pub fn received(&self, destination: &DestinationName, body: &[u8]) -> Result<Received, Unread> {
        let value: Value = serde_json::from_slice(body).map_err(|_| Unread::NotAnInteraction)?;
        self.read(destination, &value)
    }

    /// What the interaction `value`, sent to `destination`, asks.
    ///
    /// # Errors
    ///
    /// As [`Interacting::received`].
    pub fn read(&self, destination: &DestinationName, value: &Value) -> Result<Received, Unread> {
        let kind = value
            .get("type")
            .and_then(Value::as_u64)
            .ok_or(Unread::NotAnInteraction)?;
        if kind == PING {
            return Ok(Received::Ping);
        }
        if text(value, "application_id") != Some(self.interactions.application.as_str()) {
            return Ok(Received::Ignored);
        }
        let data = value.get("data").ok_or(Unread::NotAnInteraction)?;
        let place = text(value, "channel_id")
            .ok_or(Unread::Unplaced)?
            .to_owned();
        let (command, given) = match kind {
            APPLICATION_COMMAND | AUTOCOMPLETE => match self.command(data) {
                Some(found) => found,
                None => return Ok(Received::Ignored),
            },
            MESSAGE_COMPONENT => {
                let Some(project) =
                    text(data, "custom_id").and_then(|id| id.strip_prefix(SUBSCRIBE))
                else {
                    return Ok(Received::Ignored);
                };
                return Self::asked(
                    destination,
                    value,
                    place,
                    Command::Subscribe,
                    Some(project.to_owned()),
                    false,
                    true,
                );
            }
            _ => return Ok(Received::Ignored),
        };
        if kind == AUTOCOMPLETE {
            let typed = given
                .iter()
                .find(|option| option.get("focused").and_then(Value::as_bool) == Some(true))
                .and_then(|option| option.get("value"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let id = text(value, "id").ok_or(Unread::NotAnInteraction)?;
            let token = text(value, "token").ok_or(Unread::NotAnInteraction)?;
            return Ok(Received::Completing {
                place,
                typed,
                replying: Replying {
                    id: id.to_owned(),
                    token: Secret::new("the interaction's token", token),
                },
            });
        }
        let project = option(given, PROJECT)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let share = option(given, SHARE).and_then(Value::as_bool) == Some(true);
        Self::asked(destination, value, place, command, project, share, false)
    }

    fn command<'a>(&self, data: &'a Value) -> Option<(Command, &'a [Value])> {
        let name = text(data, "name")?;
        match &self.interactions.group {
            Some(group) if group == name => {
                let sub = options(data).iter().find(|option| {
                    option.get("type").and_then(Value::as_u64) == Some(SUB_COMMAND)
                })?;
                Some((Command::named(text(sub, "name")?)?, options(sub)))
            }
            Some(_) => None,
            None => Some((Command::named(name)?, options(data))),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn asked(
        destination: &DestinationName,
        value: &Value,
        place: String,
        command: Command,
        project: Option<String>,
        share: bool,
        toggles: bool,
    ) -> Result<Received, Unread> {
        let id = text(value, "id").ok_or(Unread::NotAnInteraction)?;
        let token = text(value, "token").ok_or(Unread::NotAnInteraction)?;
        let member = value.get("member");
        let user = member
            .and_then(|member| member.get("user"))
            .or_else(|| value.get("user"))
            .and_then(|user| text(user, "id"))
            .ok_or(Unread::Unplaced)?;
        let member = match (member, text(value, "guild_id")) {
            (Some(member), Some(guild)) => Some(Member {
                guild: guild.to_owned(),
                roles: member
                    .get("roles")
                    .and_then(Value::as_array)
                    .map(|roles| {
                        roles
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
            }),
            _ => None,
        };
        Ok(Received::Asked {
            asked: Asked {
                destination: destination.clone(),
                place,
                user: user.to_owned(),
                command,
                project,
                share,
                toggles,
                member,
            },
            replying: Replying {
                id: id.to_owned(),
                token: Secret::new("the interaction's token", token),
            },
        })
    }

    /// The commands as Discord registers them, each with the options it
    /// takes: as commands of their own, or as subcommands of the group.
    #[must_use]
    pub fn commands(&self) -> Value {
        let described: Vec<Value> = self
            .interactions
            .commands
            .iter()
            .map(|command| {
                let mut options = vec![json!({
                    "type": STRING,
                    "name": PROJECT,
                    "description": "The project, where more than one is announced here",
                    "required": false,
                    "autocomplete": true,
                })];
                if command.shares() {
                    options.push(json!({
                        "type": BOOLEAN,
                        "name": SHARE,
                        "description": "Show the answer to the channel",
                        "required": false,
                    }));
                }
                json!({
                    "name": command.name(),
                    "description": command.description(),
                    "options": options,
                })
            })
            .collect();
        match &self.interactions.group {
            Some(group) => json!([{
                "name": group,
                "description": "Ask herald about the projects it announces",
                "options": described
                    .into_iter()
                    .map(|mut sub| {
                        if let Some(object) = sub.as_object_mut() {
                            object.insert("type".to_owned(), json!(SUB_COMMAND));
                        }
                        sub
                    })
                    .collect::<Vec<Value>>(),
            }]),
            None => Value::Array(described),
        }
    }

    fn bare(&self, method: Method, path: &str) -> Option<Request> {
        let bot = self.bot.clone()?;
        Some(
            Request::new(method, Text::plain(format!("{}{path}", self.api)))
                .header("User-Agent", Text::plain(crate::request::USER_AGENT))
                .header("Authorization", Text::plain("Bot ").then_secret(bot)),
        )
    }

    fn json(&self, method: Method, path: &str, body: &Value) -> Option<Request> {
        Some(
            self.bare(method, path)?
                .header("Content-Type", Text::plain("application/json"))
                .body(body.to_string()),
        )
    }

    /// The request that registers the commands, in the server the
    /// interactions name or in every server, replacing any the application
    /// registered before; none without a bot token.
    #[must_use]
    pub fn register(&self) -> Option<Request> {
        let application = &self.interactions.application;
        let path = match &self.interactions.guild {
            Some(guild) => format!("/applications/{application}/guilds/{guild}/commands"),
            None => format!("/applications/{application}/commands"),
        };
        self.json(Method::Put, &path, &self.commands())
    }

    /// The request that gives `user` the role `role` in `guild` where `give`,
    /// or takes it away; none without a bot token or where an id is not
    /// written as Discord writes one.
    #[must_use]
    pub fn role(&self, guild: &str, user: &str, role: &str, give: bool) -> Option<Request> {
        if ![guild, user, role].into_iter().all(snowflake) {
            return None;
        }
        let method = if give { Method::Put } else { Method::Delete };
        Some(
            self.bare(
                method,
                &format!("/guilds/{guild}/members/{user}/roles/{role}"),
            )?
            .header(
                "X-Audit-Log-Reason",
                Text::plain(if give {
                    "Subscribed through herald"
                } else {
                    "Unsubscribed through herald"
                }),
            ),
        )
    }

    /// The request listing the roles of `guild`; none without a bot token or
    /// where the id is not written as Discord writes one.
    #[must_use]
    pub fn roles(&self, guild: &str) -> Option<Request> {
        if !snowflake(guild) {
            return None;
        }
        self.bare(Method::Get, &format!("/guilds/{guild}/roles"))
    }

    /// The request that makes a mentionable role named `name` in `guild`;
    /// none without a bot token or where the id is not written as Discord
    /// writes one.
    #[must_use]
    pub fn make_role(&self, guild: &str, name: &str) -> Option<Request> {
        if !snowflake(guild) {
            return None;
        }
        self.json(
            Method::Post,
            &format!("/guilds/{guild}/roles"),
            &json!({ "name": name, "mentionable": true }),
        )
    }

    fn hook(&self, replying: &Replying, rest: &str) -> Text {
        Text::plain(format!(
            "{}/webhooks/{}/",
            self.api, self.interactions.application
        ))
        .then_secret(replying.token.clone())
        .then(rest)
    }

    /// The request that answers the interaction `replying` names as one that
    /// is being worked, seen by the asker alone unless `public`: the answer
    /// an endpoint gives in its response, sent where the gateway carried the
    /// interaction.
    #[must_use]
    pub fn defer(&self, replying: &Replying, public: bool) -> Request {
        Request::new(
            Method::Post,
            Text::plain(format!("{}/interactions/{}/", self.api, replying.id))
                .then_secret(replying.token.clone())
                .then("/callback"),
        )
        .header("Content-Type", Text::plain("application/json"))
        .header("User-Agent", Text::plain(crate::request::USER_AGENT))
        .body(deferred(public))
    }

    /// The request that answers the interaction `replying` names with
    /// `projects` as the completions of what was typed, sent where the
    /// gateway carried the interaction.
    #[must_use]
    pub fn complete(&self, replying: &Replying, projects: &[String]) -> Request {
        Request::new(
            Method::Post,
            Text::plain(format!("{}/interactions/{}/", self.api, replying.id))
                .then_secret(replying.token.clone())
                .then("/callback"),
        )
        .header("Content-Type", Text::plain("application/json"))
        .header("User-Agent", Text::plain(crate::request::USER_AGENT))
        .body(choices(projects))
    }

    /// The requests that answer the interaction `replying` names with
    /// `parts`, each within [`MESSAGE`]: the first in place of the answer
    /// that said herald was working, each other as a follow-up, seen by the
    /// asker alone unless `public`. No answer mentions anyone.
    #[must_use]
    pub fn answered(&self, replying: &Replying, parts: &[String], public: bool) -> Vec<Request> {
        let flags = if public { 0 } else { EPHEMERAL };
        parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                let (method, rest, body) = if index == 0 {
                    (
                        Method::Patch,
                        "/messages/@original",
                        json!({ "content": part, "allowed_mentions": { "parse": [] } }),
                    )
                } else {
                    (
                        Method::Post,
                        "",
                        json!({
                            "content": part,
                            "flags": flags,
                            "allowed_mentions": { "parse": [] },
                        }),
                    )
                };
                Request::new(method, self.hook(replying, rest))
                    .header("Content-Type", Text::plain("application/json"))
                    .header("User-Agent", Text::plain(crate::request::USER_AGENT))
                    .body(body.to_string())
            })
            .collect()
    }
}

/// The response that answers a ping.
#[must_use]
pub fn pong() -> String {
    json!({ "type": PONG }).to_string()
}

/// The response that says herald is working on an interaction, seen by the
/// asker alone unless `public`.
#[must_use]
pub fn deferred(public: bool) -> String {
    if public {
        json!({ "type": DEFERRED }).to_string()
    } else {
        json!({ "type": DEFERRED, "data": { "flags": EPHEMERAL } }).to_string()
    }
}

/// The response that lists `projects` as the completions of what a member
/// typed.
#[must_use]
pub fn choices(projects: &[String]) -> String {
    let choices: Vec<Value> = projects
        .iter()
        .map(|project| json!({ "name": project, "value": project }))
        .collect();
    json!({ "type": CHOICES, "data": { "choices": choices } }).to_string()
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
pub fn took(response: &Response) -> Result<(), String> {
    if response.succeeded() {
        return Ok(());
    }
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    let message = text(&body, "message").unwrap_or_default();
    Err(format!("Discord answered {}: {message}", response.status)
        .trim_end_matches([' ', ':'])
        .to_owned())
}

/// The roles a `GET /guilds/{guild}/roles` answer lists, each as its id and
/// name.
///
/// # Errors
///
/// What Discord said, where it did not list them.
pub fn listed(response: &Response) -> Result<Vec<(String, String)>, String> {
    took(response)?;
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    Ok(body
        .as_array()
        .map(|roles| {
            roles
                .iter()
                .filter_map(|role| {
                    Some((text(role, "id")?.to_owned(), text(role, "name")?.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default())
}

/// The role a `POST /guilds/{guild}/roles` answer made, as its id and name.
///
/// # Errors
///
/// What Discord said, where it made none.
pub fn made(response: &Response) -> Result<(String, String), String> {
    took(response)?;
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    match (text(&body, "id"), text(&body, "name")) {
        (Some(id), Some(name)) => Ok((id.to_owned(), name.to_owned())),
        _ => Err("Discord answered with no role".to_owned()),
    }
}

#[cfg(test)]
mod tests;
