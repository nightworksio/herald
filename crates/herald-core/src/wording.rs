use std::collections::BTreeMap;

use jiff::Timestamp;
use jiff::fmt::strtime;
use minijinja::{Environment, UndefinedBehavior, context};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::bounds::Sandbox;
use crate::interact::{Command, Said, Unchosen};
use crate::markup::escaped;

/// One thing herald says where it takes commands, as `wording` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Phrase {
    /// A member is now subscribed to `project`.
    Subscribed,
    /// A member was subscribed to `project` already.
    AlreadySubscribed,
    /// A member is no longer subscribed to `project`.
    Unsubscribed,
    /// A member was not subscribed to `project`.
    NotSubscribed,
    /// Subscribing to `project` is not offered where it was asked.
    Unoffered,
    /// Nothing has been announced where a command was given.
    Nothing,
    /// More than one of `projects` is announced there.
    Many,
    /// `named` is not announced there; `projects` are.
    Unknown,
    /// `command` is not answered there.
    Disabled,
    /// herald could not answer.
    Unavailable,
    /// `changelog` found no notes.
    NoNotes,
    /// The link to the release, after `changelog`'s notes.
    ReleaseLink,
    /// The link to the changelog, after `changelog`'s notes.
    ChangelogLink,
    /// How `latest` writes the day a release happened, as a strftime
    /// pattern.
    Date,
    /// What `subscribe` does, as a destination lists it.
    DescribeSubscribe,
    /// What `unsubscribe` does.
    DescribeUnsubscribe,
    /// What `latest` does.
    DescribeLatest,
    /// What `changelog` does.
    DescribeChangelog,
    /// What the group of commands does, where they are grouped.
    DescribeGroup,
    /// What a command's `project` option names.
    DescribeProject,
    /// What a command's `share` option does.
    DescribeShare,
    /// The label of a post's subscribe button.
    SubscribeButton,
    /// Why a member was given a project's role, as an audit log records it.
    AuditSubscribed,
    /// Why a member's role was taken away, as an audit log records it.
    AuditUnsubscribed,
}

impl Phrase {
    /// The longest description a destination lists: Discord's.
    pub const DESCRIPTION: usize = 100;
    /// The longest label a button takes: Slack's.
    pub const LABEL: usize = 75;

    /// What herald says where the config words nothing.
    #[must_use]
    pub const fn default_text(self) -> &'static str {
        match self {
            Self::Subscribed => "You will be mentioned when {{ project }} announces a release.",
            Self::AlreadySubscribed => {
                "You are already mentioned when {{ project }} announces a release."
            }
            Self::Unsubscribed => {
                "You will no longer be mentioned when {{ project }} announces a release."
            }
            Self::NotSubscribed => "You are not mentioned when {{ project }} announces a release.",
            Self::Unoffered => "Subscribing to {{ project }} is not offered here.",
            Self::Nothing => "Nothing has been announced here yet.",
            Self::Many => "More than one project is announced here; name one of {{ projects }}.",
            Self::Unknown => "{{ named }} is not announced here; name one of {{ projects }}.",
            Self::Disabled => "The command {{ command }} is not answered here.",
            Self::Unavailable => "herald could not answer just now; try again later.",
            Self::NoNotes => "No notes were announced.",
            Self::ReleaseLink => "Release",
            Self::ChangelogLink => "Changelog",
            Self::Date => "%Y-%m-%d",
            Self::DescribeSubscribe => "Be mentioned when a project announces a release",
            Self::DescribeUnsubscribe => "Stop being mentioned when a project announces a release",
            Self::DescribeLatest => "Show what a project announced last",
            Self::DescribeChangelog => "Show the notes of what a project announced last",
            Self::DescribeGroup => "Ask herald about the projects it announces",
            Self::DescribeProject => "The project, where more than one is announced here",
            Self::DescribeShare => "Show the answer to the channel",
            Self::SubscribeButton => "Subscribe",
            Self::AuditSubscribed => "Subscribed through herald",
            Self::AuditUnsubscribed => "Unsubscribed through herald",
        }
    }

    /// Its name, as `wording` writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Subscribed => "subscribed",
            Self::AlreadySubscribed => "already_subscribed",
            Self::Unsubscribed => "unsubscribed",
            Self::NotSubscribed => "not_subscribed",
            Self::Unoffered => "unoffered",
            Self::Nothing => "nothing",
            Self::Many => "many",
            Self::Unknown => "unknown",
            Self::Disabled => "disabled",
            Self::Unavailable => "unavailable",
            Self::NoNotes => "no_notes",
            Self::ReleaseLink => "release_link",
            Self::ChangelogLink => "changelog_link",
            Self::Date => "date",
            Self::DescribeSubscribe => "describe_subscribe",
            Self::DescribeUnsubscribe => "describe_unsubscribe",
            Self::DescribeLatest => "describe_latest",
            Self::DescribeChangelog => "describe_changelog",
            Self::DescribeGroup => "describe_group",
            Self::DescribeProject => "describe_project",
            Self::DescribeShare => "describe_share",
            Self::SubscribeButton => "subscribe_button",
            Self::AuditSubscribed => "audit_subscribed",
            Self::AuditUnsubscribed => "audit_unsubscribed",
        }
    }

    /// Every phrase.
    pub const ALL: [Self; 24] = [
        Self::Subscribed,
        Self::AlreadySubscribed,
        Self::Unsubscribed,
        Self::NotSubscribed,
        Self::Unoffered,
        Self::Nothing,
        Self::Many,
        Self::Unknown,
        Self::Disabled,
        Self::Unavailable,
        Self::NoNotes,
        Self::ReleaseLink,
        Self::ChangelogLink,
        Self::Date,
        Self::DescribeSubscribe,
        Self::DescribeUnsubscribe,
        Self::DescribeLatest,
        Self::DescribeChangelog,
        Self::DescribeGroup,
        Self::DescribeProject,
        Self::DescribeShare,
        Self::SubscribeButton,
        Self::AuditSubscribed,
        Self::AuditUnsubscribed,
    ];

    /// The most characters a destination takes of it, where it is a
    /// description or a label.
    const fn longest(self) -> Option<usize> {
        match self {
            Self::DescribeSubscribe
            | Self::DescribeUnsubscribe
            | Self::DescribeLatest
            | Self::DescribeChangelog
            | Self::DescribeGroup
            | Self::DescribeProject
            | Self::DescribeShare => Some(Self::DESCRIPTION),
            Self::ReleaseLink | Self::ChangelogLink | Self::SubscribeButton => Some(Self::LABEL),
            _ => None,
        }
    }
}

/// What herald says where it takes commands: each phrase as the config
/// words it, a template over `project`, `projects`, `named` and `command`,
/// or as herald words it by default.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Wording {
    worded: BTreeMap<Phrase, String>,
}

impl Wording {
    /// The wording `worded` gives, herald's own for every phrase it leaves
    /// out.
    #[must_use]
    pub fn of(worded: Option<&BTreeMap<Phrase, String>>) -> Self {
        Self {
            worded: worded.cloned().unwrap_or_default(),
        }
    }

    /// Whether every phrase `worded` gives can be said.
    ///
    /// # Errors
    ///
    /// The first that cannot, naming it and why.
    pub fn check(worded: &BTreeMap<Phrase, String>) -> Result<(), String> {
        let environment = environment();
        for (phrase, text) in worded {
            let name = phrase.name();
            if *phrase == Phrase::Date {
                strtime::format(text, Timestamp::UNIX_EPOCH).map_err(|error| {
                    format!("wording.{name} is not a strftime pattern: {error}")
                })?;
                continue;
            }
            if let Some(most) = phrase.longest()
                && !(1..=most).contains(&text.chars().count())
            {
                return Err(format!("wording.{name} takes 1 to {most} characters"));
            }
            environment
                .template_from_str(text)
                .map_err(|error| format!("wording.{name} is not a template: {error}"))?;
        }
        Ok(())
    }

    /// The text of `phrase`, as the config words it or herald does.
    #[must_use]
    pub fn text(&self, phrase: Phrase) -> &str {
        self.worded
            .get(&phrase)
            .map_or(phrase.default_text(), String::as_str)
    }

    /// What herald says back, `said` as this wording words it.
    #[must_use]
    pub fn say(&self, said: &Said) -> String {
        let one =
            |phrase, project: &str| self.render(phrase, &context! { project => escaped(project) });
        match said {
            Said::Subscribed(project) => one(Phrase::Subscribed, project),
            Said::AlreadySubscribed(project) => one(Phrase::AlreadySubscribed, project),
            Said::Unsubscribed(project) => one(Phrase::Unsubscribed, project),
            Said::NotSubscribed(project) => one(Phrase::NotSubscribed, project),
            Said::Unoffered(project) => one(Phrase::Unoffered, project),
            Said::Unchosen(unchosen) => self.unchosen(unchosen),
            Said::Disabled(command) => {
                self.render(Phrase::Disabled, &context! { command => command.name() })
            }
            Said::Unavailable => self.render(Phrase::Unavailable, &context! {}),
        }
    }

    /// What `command` does, as a destination lists it.
    #[must_use]
    pub fn describe(&self, command: Command) -> &str {
        self.text(match command {
            Command::Subscribe => Phrase::DescribeSubscribe,
            Command::Unsubscribe => Phrase::DescribeUnsubscribe,
            Command::Latest => Phrase::DescribeLatest,
            Command::Changelog => Phrase::DescribeChangelog,
        })
    }

    /// The day `at`, as `latest` writes it.
    #[must_use]
    pub fn date(&self, at: Timestamp) -> String {
        strtime::format(self.text(Phrase::Date), at)
            .or_else(|_| strtime::format(Phrase::Date.default_text(), at))
            .unwrap_or_default()
    }

    fn unchosen(&self, unchosen: &Unchosen) -> String {
        let listed = |known: &[String]| {
            known
                .iter()
                .map(|project| escaped(project))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match unchosen {
            Unchosen::Nothing => self.render(Phrase::Nothing, &context! {}),
            Unchosen::Many(known) => {
                self.render(Phrase::Many, &context! { projects => listed(known) })
            }
            Unchosen::Unknown { named, known } => self.render(
                Phrase::Unknown,
                &context! { named => escaped(named), projects => listed(known) },
            ),
        }
    }

    /// `phrase` rendered with `values`, or herald's own text where the
    /// config's cannot be.
    fn render(&self, phrase: Phrase, values: &minijinja::Value) -> String {
        let environment = environment();
        environment
            .render_str(self.text(phrase), values)
            .or_else(|_| environment.render_str(phrase.default_text(), values))
            .unwrap_or_else(|_| phrase.default_text().to_owned())
    }
}

/// The environment wording renders in: herald's template sandbox, with an
/// undefined value written as nothing.
fn environment() -> Environment<'static> {
    let mut environment = Environment::new();
    environment.set_fuel(Some(Sandbox::DEFAULT.fuel));
    environment.set_recursion_limit(Sandbox::DEFAULT.recursion);
    environment.set_undefined_behavior(UndefinedBehavior::Lenient);
    environment
}

#[cfg(test)]
mod tests;
