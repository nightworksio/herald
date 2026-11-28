//! How a Teams bot takes what Teams posts to it, as
//! `[destinations.*.interactions]` writes it (ADR-0016, ADR-0093).

use std::collections::BTreeMap;

use herald_core::{Answering, Command, Phrase, Replies, TemplateName, Wording, every_command};
use schemars::JsonSchema;
use serde::Deserialize;

/// How Teams carries what it posts to a bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "TeamsVia")]
pub enum Via {
    /// Teams posts each activity to herald's endpoint with a bearer token
    /// the Bot Framework signs: served by `herald server` and by a
    /// serverless driver.
    #[default]
    Http,
}

/// How a Teams bot takes what Teams posts to it, and answers the commands
/// members give it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TeamsInteractions")]
pub struct Interactions {
    /// How Teams carries it.
    #[serde(default)]
    pub via: Via,
    /// The commands it answers, each given to the bot in a message: after
    /// its mention in a channel or a group chat, `@herald latest herald`,
    /// and alone in a chat with the bot. Each answer replies to the
    /// member's message, where everyone in the conversation reads it.
    #[serde(default = "every_command")]
    pub commands: Vec<Command>,
    /// Whether `latest` and `changelog` answer with a pre-release newer than
    /// every release.
    #[serde(default)]
    pub prereleases: bool,
    /// The template `latest` takes its title from and `changelog` its body,
    /// in place of the route's own.
    pub template: Option<String>,
    /// What it says back, phrase by phrase, each a template over `project`,
    /// `projects`, `named` and `command`; herald's own words for every
    /// phrase left out.
    pub wording: Option<BTreeMap<Phrase, String>>,
}

impl Default for Interactions {
    fn default() -> Self {
        Self {
            via: Via::default(),
            commands: every_command(),
            prereleases: false,
            template: None,
            wording: None,
        }
    }
}

impl Interactions {
    /// How it answers: in the conversation, whatever a member asks to
    /// share, as Teams shows a bot's reply to everyone there.
    #[must_use]
    pub fn answering(&self) -> Answering {
        Answering {
            commands: self.commands.clone(),
            replies: Replies::Public,
            prereleases: self.prereleases,
            template: self.template.as_deref().map(TemplateName::new),
            wording: Wording::of(self.wording.as_ref()),
        }
    }

    /// Whether its wording reads and its commands are named once each.
    ///
    /// # Errors
    ///
    /// The first that is not, naming the fix.
    pub fn check(&self) -> Result<(), String> {
        if let Some(worded) = &self.wording {
            Wording::check(worded)?;
        }
        Answering::check(&self.commands)
    }
}
