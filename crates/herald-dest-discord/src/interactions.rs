//! How a Discord destination takes commands, as
//! `[destinations.*.interactions]` writes it (ADR-0016).

use herald_core::{Answering, Command, Replies, TemplateName, every_command};
use schemars::JsonSchema;
use serde::Deserialize;

/// How Discord carries commands to herald.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Via {
    /// Discord posts each interaction to herald's endpoint, signed with the
    /// application's key: served by `herald server` and by a serverless
    /// driver.
    #[default]
    Http,
    /// herald holds a connection to Discord's gateway and takes each
    /// interaction from it: served by `herald server`, with no public
    /// endpoint.
    Gateway,
}

/// How a Discord destination takes commands.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Interactions {
    /// How Discord carries them.
    #[serde(default)]
    pub via: Via,
    /// The application's id, from its General Information page.
    pub application: String,
    /// The application's public key, in hex, from the same page. Each
    /// interaction posted to herald is checked against it.
    pub key: Option<String>,
    /// The commands it answers.
    #[serde(default = "every_command")]
    pub commands: Vec<Command>,
    /// Registers the commands as subcommands of `/<group>`, such as
    /// `/herald latest`, in place of commands of their own.
    pub group: Option<String>,
    /// The server the commands are registered in alone, by id; where none is
    /// named, they are registered in every server the application is in.
    pub guild: Option<String>,
    /// Who sees the answers to `latest` and `changelog`.
    #[serde(default)]
    pub replies: Replies,
    /// Whether `latest` and `changelog` answer with a pre-release newer than
    /// every release.
    #[serde(default)]
    pub prereleases: bool,
    /// The template `latest` takes its title from and `changelog` its body,
    /// in place of the route's own.
    pub template: Option<String>,
}

impl Interactions {
    /// How it answers, whatever carries the commands.
    #[must_use]
    pub fn answering(&self) -> Answering {
        Answering {
            commands: self.commands.clone(),
            replies: self.replies,
            prereleases: self.prereleases,
            template: self.template.as_deref().map(TemplateName::new),
        }
    }

    /// Whether the application, its key, its group and its server are
    /// written as Discord writes them, and its commands named once each.
    ///
    /// # Errors
    ///
    /// The first that is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        if !snowflake(&self.application) {
            return Err(format!(
                "\"{}\" is not an application id; give the id from the application's General Information page",
                self.application
            ));
        }
        match (&self.key, self.via) {
            (None, Via::Http) => {
                return Err(
                    "Discord signs each interaction it posts with the application's public key; give it as key = \"…\", or take them through the gateway with via = \"gateway\"".to_owned(),
                );
            }
            (Some(key), _) if !public_key(key) => {
                return Err(format!(
                    "\"{key}\" is not a public key; give the 64 hex digits of the application's public key"
                ));
            }
            _ => {}
        }
        if let Some(group) = &self.group
            && !command_name(group)
        {
            return Err(format!(
                "\"{group}\" cannot name a command; use 1 to 32 lowercase letters, digits, - or _"
            ));
        }
        if let Some(guild) = &self.guild
            && !snowflake(guild)
        {
            return Err(format!(
                "\"{guild}\" is not a server id; give the server's id"
            ));
        }
        Answering::check(&self.commands)
    }
}

/// What a route says about subscribing on Discord.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Subscribe {
    /// The opt-in role `subscribe` gives a member and `unsubscribe` takes
    /// away, by id. The route mentions it with its announcements.
    pub role: String,
    /// Whether each announcement carries a button that subscribes, or
    /// unsubscribes a member already subscribed.
    #[serde(default = "yes")]
    pub button: bool,
}

const fn yes() -> bool {
    true
}

impl Subscribe {
    /// Whether the role is written as a role id.
    ///
    /// # Errors
    ///
    /// When it is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        if snowflake(&self.role) {
            Ok(())
        } else {
            Err(format!(
                "\"{}\" is not a role id; give the role's id, or make one with herald discord roles",
                self.role
            ))
        }
    }
}

/// Whether `id` is written as Discord writes an id: 17 to 20 digits.
pub(crate) fn snowflake(id: &str) -> bool {
    (17..=20).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_digit())
}

fn public_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Whether `name` may name a Discord command.
pub(crate) fn command_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

#[cfg(test)]
mod tests;
