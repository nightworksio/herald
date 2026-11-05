//! What a member asks herald in a channel and what herald answers, apart
//! from how any destination carries either (ADR-0016).

use std::fmt;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::key::RouteId;
use crate::latest::Latest;
use crate::route::{DestinationName, TemplateName};

/// The most projects a destination lists as completions of what a member
/// typed: Discord's own limit for autocomplete choices.
pub const MOST_COMPLETIONS: usize = 25;

/// A command a member gives herald.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Command {
    /// Be mentioned when the project announces.
    Subscribe,
    /// Stop being mentioned when the project announces.
    Unsubscribe,
    /// What the project announced last.
    Latest,
    /// The notes of what the project announced last.
    Changelog,
}

impl Command {
    /// Every command, in the order a destination lists them.
    pub const ALL: [Self; 4] = [
        Self::Subscribe,
        Self::Unsubscribe,
        Self::Latest,
        Self::Changelog,
    ];

    /// The command's name, as a member types it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Subscribe => "subscribe",
            Self::Unsubscribe => "unsubscribe",
            Self::Latest => "latest",
            Self::Changelog => "changelog",
        }
    }

    /// The command named `name`, in any case.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|command| command.name().eq_ignore_ascii_case(name))
    }

    /// What the command does, as a destination lists it.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Subscribe => "Be mentioned when a project announces a release",
            Self::Unsubscribe => "Stop being mentioned when a project announces a release",
            Self::Latest => "Show what a project announced last",
            Self::Changelog => "Show the notes of what a project announced last",
        }
    }

    /// Whether a member may ask for the command's answer to be shown to the
    /// channel.
    #[must_use]
    pub const fn shares(self) -> bool {
        matches!(self, Self::Latest | Self::Changelog)
    }
}

/// Who sees herald's answers to `latest` and `changelog`; the answers to
/// `subscribe` and `unsubscribe` are always the asker's alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Replies {
    /// The asker alone, unless they ask to share it.
    #[default]
    Ephemeral,
    /// The channel.
    Public,
}

impl Replies {
    /// Whether the answer to `command` is shown to the channel, where the
    /// asker asked to `share` it or not.
    #[must_use]
    pub const fn public(self, command: Command, share: bool) -> bool {
        command.shares() && (share || matches!(self, Self::Public))
    }
}

/// Every command, as a destination answers by default.
#[must_use]
pub fn every_command() -> Vec<Command> {
    Command::ALL.to_vec()
}

/// How a destination answers commands, whatever carries them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answering {
    /// The commands it answers.
    pub commands: Vec<Command>,
    /// Who sees its answers to `latest` and `changelog`.
    pub replies: Replies,
    /// Whether `latest` and `changelog` answer with a pre-release that is
    /// newer than every release.
    pub prereleases: bool,
    /// The template `latest` takes its title from and `changelog` its body,
    /// in place of the route's own.
    pub template: Option<TemplateName>,
}

impl Answering {
    /// Whether it answers `command`.
    #[must_use]
    pub fn takes(&self, command: Command) -> bool {
        self.commands.contains(&command)
    }

    /// Whether `commands` names at least one command, each once.
    ///
    /// # Errors
    ///
    /// When it names none, or one twice, naming the fix.
    pub fn check(commands: &[Command]) -> Result<(), String> {
        if commands.is_empty() {
            return Err(
                "it answers no command; name at least one, or leave out the interactions"
                    .to_owned(),
            );
        }
        for (index, command) in commands.iter().enumerate() {
            if commands
                .iter()
                .take(index)
                .any(|earlier| earlier == command)
            {
                return Err(format!("it names {} twice; name it once", command.name()));
            }
        }
        Ok(())
    }
}

/// What the member asking holds where a destination keeps subscriptions as
/// roles: Discord's server and the roles the member holds in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The server.
    pub guild: String,
    /// The roles the member holds.
    pub roles: Vec<String>,
}

/// A command a member gave, as a destination read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The destination it was given on.
    pub destination: DestinationName,
    /// The channel or room it was given in.
    pub place: String,
    /// The member who gave it.
    pub user: String,
    /// The command.
    pub command: Command,
    /// The project it names, if it names one.
    pub project: Option<String>,
    /// Whether the member asked for the answer to be shown to the channel.
    pub share: bool,
    /// The member's roles, where the destination keeps subscriptions as
    /// roles.
    pub member: Option<Member>,
}

/// Why no project could be taken for a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unchosen {
    /// Nothing has been announced in the place.
    Nothing,
    /// More than one project has been announced there, and none was named.
    Many(Vec<String>),
    /// The project named has not been announced there.
    Unknown {
        /// The project named.
        named: String,
        /// The projects announced there.
        known: Vec<String>,
    },
}

impl fmt::Display for Unchosen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nothing => f.write_str("Nothing has been announced here yet."),
            Self::Many(known) => write!(
                f,
                "More than one project is announced here; name one of {}.",
                known.join(", ")
            ),
            Self::Unknown { named, known } => write!(
                f,
                "{named} is not announced here; name one of {}.",
                known.join(", ")
            ),
        }
    }
}

/// What has been announced on any of `routes`, by project name.
#[must_use]
pub fn announced<'a>(latests: &'a [Latest], routes: &[RouteId]) -> Vec<&'a Latest> {
    let mut found: Vec<&Latest> = latests
        .iter()
        .filter(|latest| routes.iter().any(|route| latest.routes.contains(route)))
        .collect();
    found.sort_by(|a, b| a.project.cmp(&b.project));
    found
}

/// The project a command acts on among `announced`: the one it `named`, in
/// any case, or the only one where it named none.
///
/// # Errors
///
/// When nothing was announced, when it named none among several, or when it
/// named one not among them.
pub fn chosen<'a>(named: Option<&str>, announced: &[&'a Latest]) -> Result<&'a Latest, Unchosen> {
    let known = || {
        announced
            .iter()
            .map(|latest| latest.project.clone())
            .collect()
    };
    match (named, announced) {
        (_, []) => Err(Unchosen::Nothing),
        (None, [only]) => Ok(only),
        (None, _) => Err(Unchosen::Many(known())),
        (Some(named), _) => announced
            .iter()
            .find(|latest| latest.project.eq_ignore_ascii_case(named))
            .copied()
            .ok_or_else(|| Unchosen::Unknown {
                named: named.to_owned(),
                known: known(),
            }),
    }
}

/// The projects among `announced` whose names hold `typed`, in any case,
/// those starting with it first, at most [`MOST_COMPLETIONS`].
#[must_use]
pub fn completions(typed: &str, announced: &[&Latest]) -> Vec<String> {
    let typed = typed.to_lowercase();
    let (mut starting, holding): (Vec<String>, Vec<String>) = announced
        .iter()
        .map(|latest| latest.project.clone())
        .filter(|project| project.to_lowercase().contains(&typed))
        .partition(|project| project.to_lowercase().starts_with(&typed));
    starting.extend(holding);
    starting.truncate(MOST_COMPLETIONS);
    starting
}

/// What herald says back to a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// The member is now subscribed to the project.
    Subscribed(String),
    /// The member was subscribed to the project already.
    AlreadySubscribed(String),
    /// The member is no longer subscribed to the project.
    Unsubscribed(String),
    /// The member was not subscribed to the project.
    NotSubscribed(String),
    /// No route announcing the project in the place mentions subscribers.
    Unoffered(String),
    /// No project could be taken.
    Unchosen(Unchosen),
    /// The destination does not take the command.
    Disabled(Command),
    /// herald could not answer, for a reason the operator reads in its log.
    Unavailable,
}

impl fmt::Display for Said {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Subscribed(project) => {
                write!(
                    f,
                    "You will be mentioned when {project} announces a release."
                )
            }
            Self::AlreadySubscribed(project) => {
                write!(
                    f,
                    "You are already mentioned when {project} announces a release."
                )
            }
            Self::Unsubscribed(project) => {
                write!(
                    f,
                    "You will no longer be mentioned when {project} announces a release."
                )
            }
            Self::NotSubscribed(project) => {
                write!(
                    f,
                    "You are not mentioned when {project} announces a release."
                )
            }
            Self::Unoffered(project) => {
                write!(f, "Subscribing to {project} is not offered here.")
            }
            Self::Unchosen(unchosen) => unchosen.fmt(f),
            Self::Disabled(command) => {
                write!(f, "The command {} is not answered here.", command.name())
            }
            Self::Unavailable => f.write_str("herald could not answer just now; try again later."),
        }
    }
}

#[cfg(test)]
mod tests;
