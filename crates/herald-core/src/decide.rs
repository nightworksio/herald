//! A closed-set decision: a question with the options the config wrote,
//! and a model's answer checked against them, unknown where it is not one
//! of them, too unsure, or missing (ADR-0057).

use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::key::Key;
use crate::queue::{Reason, Row, Turn};

/// The most options a question offers.
pub const MOST_OPTIONS: usize = 32;

/// The longest an option's key is, in bytes.
pub const LONGEST_OPTION: usize = 64;

/// The options a question may be answered with: each a key, and what it
/// means, which a model reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options(BTreeMap<String, String>);

impl Options {
    /// `options`, where there are from 2 to [`MOST_OPTIONS`], each key
    /// lowercase letters, digits, `-` and `_` of at most
    /// [`LONGEST_OPTION`] bytes, each meaning something.
    ///
    /// # Errors
    ///
    /// What is wrong with them.
    pub fn new(options: BTreeMap<String, String>) -> Result<Self, String> {
        if !(2..=MOST_OPTIONS).contains(&options.len()) {
            return Err(format!(
                "a question offers from 2 to {MOST_OPTIONS} options"
            ));
        }
        for (key, meaning) in &options {
            let keyed = !key.is_empty()
                && key.len() <= LONGEST_OPTION
                && key.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'-'
                        || byte == b'_'
                });
            if !keyed {
                return Err(format!(
                    "\"{key}\" is no option's key; write lowercase letters, digits, '-' and '_', at most {LONGEST_OPTION} of them"
                ));
            }
            if meaning.trim().is_empty() {
                return Err(format!(
                    "the option \"{key}\" says nothing of what it means"
                ));
            }
        }
        Ok(Self(options))
    }

    /// Each option's key and what it means, by key.
    #[must_use]
    pub const fn as_map(&self) -> &BTreeMap<String, String> {
        &self.0
    }

    /// Whether `key` is one of them.
    #[must_use]
    pub fn offers(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
}

/// One question a decider is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Question<'a> {
    /// What names it among those asked together.
    pub id: &'a str,
    /// What to decide, in a sentence the config wrote.
    pub instructions: &'a str,
    /// The options it may be answered with.
    pub options: &'a Options,
}

/// What a model answered one question, as herald records it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    /// The model that answered, as it named itself.
    pub model: String,
    /// The option it chose, as it wrote it.
    pub choice: Option<String>,
    /// The probability it gave each option, where it gave any.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub probabilities: BTreeMap<String, f64>,
}

impl Answer {
    /// The option it settles `question` on: one the question offers, whose
    /// probability is at least `least` where `least` is given; none, as
    /// unknown, otherwise, and where `least` is given and the model gave
    /// no probability for it.
    #[must_use]
    pub fn settled<'q>(&self, question: &Question<'q>, least: Option<f64>) -> Option<&'q str> {
        let choice = self.choice.as_deref()?;
        let (key, _) = question.options.0.get_key_value(choice)?;
        let sure = least.is_none_or(|least| {
            self.probabilities
                .get(choice)
                .is_some_and(|probability| *probability >= least)
        });
        sure.then_some(key.as_str())
    }
}

/// A decision herald recorded: what a model answered a task of one
/// subject shown as one input, reused while that input stands (ADR-0057).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decided {
    /// Its key: the task's, the subject's and the input's.
    pub key: Key,
    /// The task asked.
    pub task: String,
    /// What it decided of, such as a pull request's identity.
    pub subject: String,
    /// What the model answered.
    pub answer: Answer,
    /// When it answered.
    pub at: Timestamp,
}

/// A closed-set decision a view may ask of a decider, where its rules
/// cannot tell.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Slot {
    /// Whose turn a pull request is, where herald's rules tell nobody's.
    Turn,
    /// How urgent a pull request is.
    Priority,
    /// Who should review a pull request nobody was asked to.
    Reviewer,
    /// Whether a stale pull request is waiting or noise.
    Noise,
    /// What kind of failure a pull request's failed check is.
    Failure,
}

impl Slot {
    /// Every slot, in the order a view asks them.
    pub const ALL: [Self; 5] = [
        Self::Turn,
        Self::Priority,
        Self::Reviewer,
        Self::Noise,
        Self::Failure,
    ];

    /// The slot as a config and a template name it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Priority => "priority",
            Self::Reviewer => "reviewer",
            Self::Noise => "noise",
            Self::Failure => "failure",
        }
    }

    /// Whether a view asks it of `row`: whose turn where its rules tell
    /// nobody's, a reviewer where it waits on review and nobody was asked,
    /// noise where it is stale, the failure's kind where a check failed,
    /// and its priority always.
    #[must_use]
    pub fn asks(self, row: &Row) -> bool {
        match self {
            Self::Turn => row.state == Turn::Unknown && row.reason != Reason::Unread,
            Self::Priority => true,
            Self::Reviewer => row.state == Turn::WaitingOnReview && row.pull.reviewers.is_empty(),
            Self::Noise => row.stale,
            Self::Failure => matches!(row.reason, Reason::CheckFailed(_)),
        }
    }

    /// Whether `options` are ones it may settle on: every state but
    /// `unknown` for whose turn it is, anything for the rest.
    #[must_use]
    pub fn takes(self, options: &Options) -> bool {
        self != Self::Turn
            || options
                .0
                .keys()
                .all(|key| Turn::named(key).is_some_and(|state| state != Turn::Unknown))
    }
}

/// What a decider is shown of `row`, as JSON: what the forge says of its
/// pull request and what herald's rules made of it, but never how long it
/// has waited, so one input stands while the pull request is unchanged.
#[must_use]
pub fn state(row: &Row) -> String {
    let pull = &row.pull;
    serde_json::json!({
        "repository": pull.repository,
        "number": pull.number,
        "title": pull.title,
        "author": pull.author,
        "bot": pull.bot,
        "draft": pull.draft,
        "labels": pull.labels,
        "reviewers": pull.reviewers,
        "state": row.state.name(),
        "reason": row.reason.written(),
        "updated": pull.updated.to_string(),
    })
    .to_string()
}

/// `row` once `slot` is settled on `choice`: its state and reason, where
/// it is whose turn it is, and else what it carries as decided.
pub fn decided(row: &mut Row, slot: Slot, (task, choice): (&str, &str)) {
    if let (Slot::Turn, Some(state)) = (slot, Turn::named(choice)) {
        row.state = state;
        row.reason = Reason::Decided(task.to_owned());
    }
    row.decided.insert(slot, choice.to_owned());
}

#[cfg(test)]
mod tests;
