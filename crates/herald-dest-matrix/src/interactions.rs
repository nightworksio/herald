//! How a Matrix destination takes commands, as
//! `[destinations.*.interactions]` writes it (ADR-0016).

use std::collections::BTreeMap;

use herald_core::{
    Answering, Command, Duration, Phrase, Replies, SecretRef, TemplateName, Wording, every_command,
};
use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::Deserialize;

/// How the homeserver carries commands to herald.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "MatrixVia")]
pub enum Via {
    /// The homeserver pushes each room event to herald as an application
    /// service, with the registration `herald matrix registration` writes:
    /// served by `herald server` and by a serverless driver.
    #[default]
    Appservice,
    /// herald syncs as the bot account and reads each message in the rooms
    /// it has joined: served by `herald server` and by a Cloudflare Worker's
    /// Durable Object, with no public endpoint and nothing for the
    /// homeserver's administrator to install.
    Sync,
}

/// How a Matrix destination takes commands.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MatrixInteractions")]
pub struct Interactions {
    /// How the homeserver carries them.
    #[serde(default)]
    pub via: Via,
    /// What a message starts with to be a command to herald:
    /// `!herald latest php-mutation-gate`.
    #[serde(default = "prefix")]
    pub prefix: String,
    /// The secret holding the token the homeserver presents with each
    /// transaction it pushes (`hs_token`).
    pub homeserver_token: Option<SecretRef>,
    /// The secret holding the token herald acts with as the application
    /// service (`as_token`).
    pub appservice_token: Option<SecretRef>,
    /// The application service's user, by its localpart.
    #[serde(default = "sender")]
    pub sender: String,
    /// The commands it answers.
    #[serde(default = "every_command")]
    pub commands: Vec<Command>,
    /// Whether answers to `latest` and `changelog` are posted to the room;
    /// Matrix has no answer seen by one member alone, so `ephemeral` answers
    /// in a thread under the command.
    #[serde(default)]
    pub replies: Replies,
    /// Whether `latest` and `changelog` answer with a pre-release newer than
    /// every release.
    #[serde(default)]
    pub prereleases: bool,
    /// The template `latest` takes its title from and `changelog` its body,
    /// in place of the route's own.
    pub template: Option<String>,
    /// What it says back and how it describes its commands, phrase by
    /// phrase, each a template over `project`, `projects`, `named` and
    /// `command`; herald's own words for every phrase left out.
    pub wording: Option<BTreeMap<Phrase, String>>,
    /// How long a sync waits for a message before it answers with none,
    /// "20s" where unset; shorter than `[http] total`, which bounds every
    /// answer.
    pub sync_wait: Option<Duration>,
    /// The most messages one sync reads from each room, from 1 to 100; 20
    /// where unset.
    pub timeline: Option<u8>,
    /// Whether the homeserver holds the application service's user to its
    /// rate limits, as its registration says; true where unset.
    #[serde(default = "rate_limited")]
    pub rate_limited: bool,
}

/// How long a sync waits where the config sets nothing.
pub(crate) const SYNC_WAIT: SignedDuration = SignedDuration::from_secs(20);
/// The messages one sync reads from each room where the config sets none.
const TIMELINE: u8 = 20;
/// The most messages one sync may read from each room.
const TIMELINE_MOST: u8 = 100;

fn prefix() -> String {
    "!herald".to_owned()
}

fn sender() -> String {
    "herald".to_owned()
}

const fn rate_limited() -> bool {
    true
}

impl Interactions {
    /// How long a sync waits for a message.
    #[must_use]
    pub fn sync_wait(&self) -> SignedDuration {
        self.sync_wait.map_or(SYNC_WAIT, |wait| wait.0)
    }

    /// The most messages one sync reads from each room.
    #[must_use]
    pub fn timeline(&self) -> u8 {
        self.timeline.unwrap_or(TIMELINE)
    }

    /// What it says, as the config words it.
    #[must_use]
    pub fn wording(&self) -> Wording {
        Wording::of(self.wording.as_ref())
    }

    /// How it answers, whatever carries the commands.
    #[must_use]
    pub fn answering(&self) -> Answering {
        Answering {
            commands: self.commands.clone(),
            replies: self.replies,
            prereleases: self.prereleases,
            template: self.template.as_deref().map(TemplateName::new),
            wording: self.wording(),
        }
    }

    /// Whether it holds what its transport needs, its prefix and sender are
    /// written soundly, and its commands are named once each.
    ///
    /// # Errors
    ///
    /// The first that is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        if let Some(worded) = &self.wording {
            Wording::check(worded)?;
        }
        if !(1..=TIMELINE_MOST).contains(&self.timeline()) {
            return Err(format!(
                "a sync reads 1 to {TIMELINE_MOST} messages from each room, not {}",
                self.timeline()
            ));
        }
        if self.via == Via::Appservice
            && (self.homeserver_token.is_none() || self.appservice_token.is_none())
        {
            return Err(
                "an application service holds two tokens; give homeserver_token = { secret = \"…\" } and appservice_token = { secret = \"…\" }, or read commands by syncing with via = \"sync\"".to_owned(),
            );
        }
        if self.prefix.is_empty() || self.prefix.chars().any(char::is_whitespace) {
            return Err(format!(
                "\"{}\" cannot start a command; give a prefix with no spaces, such as \"!herald\"",
                self.prefix
            ));
        }
        if !localpart(&self.sender) {
            return Err(format!(
                "\"{}\" is not a localpart; give the user's name before the colon, in lowercase letters, digits, ., _, =, - or /",
                self.sender
            ));
        }
        Answering::check(&self.commands)
    }
}

fn localpart(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'=' | b'-' | b'/')
        })
}

#[cfg(test)]
mod tests;
