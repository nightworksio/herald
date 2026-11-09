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
    /// What it says, as the config words it.
    pub wording: crate::wording::Wording,
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

/// Where a command was given: a channel, a thread or a room, and the
/// channel a thread is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The channel, thread or room.
    pub id: String,
    /// The channel it is a thread of, where it is one.
    pub parent: Option<String>,
}

impl Place {
    /// A channel or room that is no thread.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            parent: None,
        }
    }

    /// Whether a route announcing in `channel` announces here: in it, or in
    /// the channel it is a thread of.
    #[must_use]
    pub fn is(&self, channel: &str) -> bool {
        self.id == channel || self.parent.as_deref() == Some(channel)
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
    /// Where it was given.
    pub place: Place,
    /// The member who gave it.
    pub user: String,
    /// The command.
    pub command: Command,
    /// The project it names, if it names one.
    pub project: Option<String>,
    /// Whether the member asked for the answer to be shown to the channel.
    pub share: bool,
    /// Whether it came from a button that subscribes, or unsubscribes a
    /// member already subscribed.
    pub toggles: bool,
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

/// A project announced where a command was given, and the first route that
/// announced it there.
#[derive(Debug, PartialEq, Eq)]
pub struct AnnouncedHere<'a, R> {
    /// What the project last announced.
    pub latest: &'a Latest,
    /// The first route that announced it here.
    pub route: &'a R,
}

impl<R> Clone for AnnouncedHere<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R> Copy for AnnouncedHere<'_, R> {}

/// What has been announced on any of `routes`, each route named by `id`,
/// with the first of them it was announced on, by project name.
#[must_use]
pub fn announced<'a, R>(
    every_latest: &'a [Latest],
    routes: &'a [R],
    id: impl Fn(&R) -> &RouteId,
) -> Vec<AnnouncedHere<'a, R>> {
    let mut found: Vec<AnnouncedHere<'a, R>> = every_latest
        .iter()
        .filter_map(|latest| {
            routes
                .iter()
                .find(|route| latest.routes.contains(id(route)))
                .map(|route| AnnouncedHere { latest, route })
        })
        .collect();
    found.sort_by(|a, b| a.latest.project.cmp(&b.latest.project));
    found
}

/// The project a command acts on among `announced`: the one it `named`, in
/// any case, or the only one where it named none.
///
/// # Errors
///
/// When nothing was announced, when it named none among several, or when it
/// named one not among them.
pub fn chosen<'a, R>(
    named: Option<&str>,
    announced: &[AnnouncedHere<'a, R>],
) -> Result<AnnouncedHere<'a, R>, Unchosen> {
    let known = || {
        announced
            .iter()
            .map(|here| here.latest.project.clone())
            .collect()
    };
    match (named, announced) {
        (_, []) => Err(Unchosen::Nothing),
        (None, [only]) => Ok(*only),
        (None, _) => Err(Unchosen::Many(known())),
        (Some(named), _) => announced
            .iter()
            .find(|here| here.latest.project.eq_ignore_ascii_case(named))
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
pub fn completions<R>(typed: &str, announced: &[AnnouncedHere<'_, R>]) -> Vec<String> {
    let typed = typed.to_lowercase();
    let (mut starting, holding): (Vec<String>, Vec<String>) = announced
        .iter()
        .map(|here| here.latest.project.clone())
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
        f.write_str(&crate::wording::Wording::default().say(self))
    }
}

#[cfg(test)]
mod tests;
