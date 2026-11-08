//! How a Slack destination takes commands, as
//! `[destinations.*.interactions]` writes it (ADR-0016).

use crate::domain::Domain;
use herald_core::{Answering, Command, Replies, SecretRef, TemplateName, every_command};
use schemars::JsonSchema;
use serde::Deserialize;

/// How Slack carries commands to herald.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Via {
    /// Slack posts each command and button press to herald's endpoint,
    /// signed with the app's signing secret: served by `herald server` and
    /// by a serverless driver.
    #[default]
    Http,
    /// herald holds a Socket Mode connection, opened with the app-level
    /// token, and takes each one from it: served by `herald server`, with no
    /// public endpoint.
    Socket,
}

/// How a Slack destination takes commands.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Interactions {
    /// How Slack carries them.
    #[serde(default)]
    pub via: Via,
    /// The secret holding the app's signing secret, which each request
    /// posted to herald is checked against.
    pub signing: Option<SecretRef>,
    /// The secret holding an app-level token (`xapp-…`) with
    /// `connections:write`, which opens Socket Mode connections.
    pub app: Option<SecretRef>,
    /// The slash command members give, followed by a command and a project:
    /// `/herald latest php-mutation-gate`.
    #[serde(default = "slash")]
    pub command: String,
    /// The commands it answers.
    #[serde(default = "every_command")]
    pub commands: Vec<Command>,
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
    /// Domains beside its Slack's own a Socket Mode session may connect
    /// under, such as a proxy's.
    #[serde(default)]
    pub domains: Vec<String>,
    /// The name its app and bot user go by in the manifest `herald setup`
    /// prints: up to 35 lowercase letters, digits, `.`, `-` or `_`, and
    /// `herald` where unset.
    pub name: Option<String>,
}

fn slash() -> String {
    "/herald".to_owned()
}

/// The longest name a Slack app takes.
const APP_NAME: usize = 35;
/// The name its app and bot user go by where the config names none.
pub(crate) const DEFAULT_NAME: &str = "herald";

/// Whether `name` can name both a Slack app and its bot user.
fn app_name(name: &str) -> bool {
    (1..=APP_NAME).contains(&name.len())
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
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

    /// Whether it holds what its transport needs, its slash command is
    /// written as Slack writes one, and its commands are named once each.
    ///
    /// # Errors
    ///
    /// The first that is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        match self.via {
            Via::Http if self.signing.is_none() => {
                return Err(
                    "Slack signs each request it posts with the app's signing secret; give it as signing = { secret = \"…\" }, or take them through Socket Mode with via = \"socket\"".to_owned(),
                );
            }
            Via::Socket if self.app.is_none() => {
                return Err(
                    "Socket Mode is opened with an app-level token; give it as app = { secret = \"…\" }".to_owned(),
                );
            }
            _ => {}
        }
        if let Some(name) = self.name.as_deref().filter(|name| !app_name(name)) {
            return Err(format!(
                "\"{name}\" is not an app name; give up to {APP_NAME} lowercase letters, digits, ., - or _, such as \"herald\""
            ));
        }
        if !slash_command(&self.command) {
            return Err(format!(
                "\"{}\" is not a slash command; give one such as \"/herald\": a / and up to 31 lowercase letters, digits, - or _",
                self.command
            ));
        }
        if let Some(unsound) = self
            .domains
            .iter()
            .find(|domain| !herald_core::domain(domain))
        {
            return Err(format!(
                "\"{unsound}\" is not a domain; give a host such as \"proxy.example\", with no scheme, port or path"
            ));
        }
        Answering::check(&self.commands)
    }

    /// The domains a Socket Mode session may connect under: `domain`'s own
    /// and those the config adds.
    #[must_use]
    pub fn session_domains(&self, domain: Domain) -> Vec<&str> {
        std::iter::once(domain.host())
            .chain(self.domains.iter().map(String::as_str))
            .collect()
    }
}

/// What a route says about subscribing on Slack, where it mentions its
/// subscribers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Subscribe {
    /// Whether each announcement carries a button that subscribes, or
    /// unsubscribes a member already subscribed.
    #[serde(default = "yes")]
    pub button: bool,
}

impl Default for Subscribe {
    fn default() -> Self {
        Self { button: true }
    }
}

const fn yes() -> bool {
    true
}

fn slash_command(command: &str) -> bool {
    command.strip_prefix('/').is_some_and(|name| {
        (1..=31).contains(&name.len())
            && name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
    })
}

#[cfg(test)]
mod tests;
