//! What an interaction Discord sent asks of herald: a ping, a command, a
//! button press, or a project's name being typed.

use std::fmt;

use herald_core::{Asked, Command, DestinationName, Member, Place};
use herald_ports::Secret;
use serde_json::Value;

use super::{Interacting, PROJECT, Replying, SHARE, SUB_COMMAND, SUBSCRIBE, text};
use crate::interactions::snowflake;

/// Discord's ping, which an endpoint answers with a pong.
const PING: u64 = 1;
/// A slash command.
const APPLICATION_COMMAND: u64 = 2;
/// A press of a message component, such as a button.
const MESSAGE_COMPONENT: u64 = 3;
/// A member typing into an option that completes.
const AUTOCOMPLETE: u64 = 4;
/// The kinds of channel that are threads: an announcement channel's, a
/// public one and a private one.
const THREADS: [u64; 3] = [10, 11, 12];

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
        /// Where it is typed.
        place: Place,
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

/// Where the interaction `value` was given: its channel, and the channel
/// that is in where it is a thread.
fn placed(value: &Value) -> Option<Place> {
    let id = text(value, "channel_id").filter(|id| snowflake(id))?;
    let channel = value.get("channel");
    let threaded = channel
        .and_then(|channel| channel.get("type"))
        .and_then(Value::as_u64)
        .is_some_and(|kind| THREADS.contains(&kind));
    let parent = channel
        .and_then(|channel| text(channel, "parent_id"))
        .filter(|parent| threaded && snowflake(parent));
    Some(Place {
        id: id.to_owned(),
        parent: parent.map(ToOwned::to_owned),
    })
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
        let place = placed(value).ok_or(Unread::Unplaced)?;
        let (command, given) = match kind {
            APPLICATION_COMMAND | AUTOCOMPLETE => match self.command(data) {
                Some(found) => found,
                None => return Ok(Received::Ignored),
            },
            MESSAGE_COMPONENT => return pressed(destination, value, place, data),
            _ => return Ok(Received::Ignored),
        };
        if kind == AUTOCOMPLETE {
            return completing(value, place, given);
        }
        let project = option(given, PROJECT)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let share = option(given, SHARE).and_then(Value::as_bool) == Some(true);
        asked(destination, value, place, command, project, share, false)
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
}

/// What the press of a button, the interaction `value` with `data`, asks:
/// herald's subscribe button toggles the subscription its custom id names.
fn pressed(
    destination: &DestinationName,
    value: &Value,
    place: Place,
    data: &Value,
) -> Result<Received, Unread> {
    let Some(project) = text(data, "custom_id").and_then(|id| id.strip_prefix(SUBSCRIBE)) else {
        return Ok(Received::Ignored);
    };
    asked(
        destination,
        value,
        place,
        Command::Subscribe,
        Some(project.to_owned()),
        false,
        true,
    )
}

/// What a member typing into the option of `given` that has the focus, in
/// the interaction `value`, completes.
fn completing(value: &Value, place: Place, given: &[Value]) -> Result<Received, Unread> {
    let typed = given
        .iter()
        .find(|option| option.get("focused").and_then(Value::as_bool) == Some(true))
        .and_then(|option| option.get("value"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(Received::Completing {
        place,
        typed,
        replying: replying(value)?,
    })
}

#[allow(clippy::too_many_arguments)]
fn asked(
    destination: &DestinationName,
    value: &Value,
    place: Place,
    command: Command,
    project: Option<String>,
    share: bool,
    toggles: bool,
) -> Result<Received, Unread> {
    let replying = replying(value)?;
    let user = value
        .get("member")
        .and_then(|member| member.get("user"))
        .or_else(|| value.get("user"))
        .and_then(|user| text(user, "id"))
        .filter(|user| snowflake(user))
        .ok_or(Unread::Unplaced)?;
    Ok(Received::Asked {
        asked: Asked {
            destination: destination.clone(),
            place,
            user: user.to_owned(),
            command,
            project,
            share,
            toggles,
            member: member(value),
        },
        replying,
    })
}

/// Where the answer to the interaction `value` goes.
fn replying(value: &Value) -> Result<Replying, Unread> {
    let id = text(value, "id").ok_or(Unread::NotAnInteraction)?;
    let token = text(value, "token").ok_or(Unread::NotAnInteraction)?;
    Ok(Replying {
        id: id.to_owned(),
        token: Secret::new("the interaction's token", token),
    })
}

/// The server member who gave the interaction `value`, with their roles,
/// where it was given in a server.
fn member(value: &Value) -> Option<Member> {
    let member = value.get("member")?;
    let guild = text(value, "guild_id")?;
    Some(Member {
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
    })
}
