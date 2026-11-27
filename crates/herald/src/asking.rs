//! How herald's wizards ask: each question answered by its flag
//! where given, by its default under `--yes`, or by the operator, who is
//! asked again where an answer is refused, up to [`MOST_REFUSED`] times.
//! A refused answer is never repeated, as it may be a credential typed in
//! the wrong place.

use std::io::Write;

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::de::value::{Error as Unread, StrDeserializer};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt as _};

use crate::output::say;

/// The refused answers to one question after which a wizard stops.
pub(crate) const MOST_REFUSED: u8 = 3;

/// Why the wizard stopped where the answers ended.
const UNANSWERED: &str =
    "the answers ended before every question was answered; nothing was written";

/// What a list's answer says for none.
pub(crate) const NONE: &str = "none";

/// Why an empty answer to a question without a default is asked again.
const NEEDED: &str = "this question has no default; give an answer";

/// One question: what it asks, the flag that answers it, the answer given
/// there, and the default.
pub(crate) struct Question<'q> {
    /// What it asks.
    pub(crate) asked: &'q str,
    /// The flag that answers it.
    pub(crate) flag: &'static str,
    /// The flag's answer, where given.
    pub(crate) given: Option<&'q str>,
    /// The answer an empty line or `--yes` takes.
    pub(crate) default: Option<String>,
}

/// The values a type of the config takes, each with its one-line
/// description, as the config's schema lists them.
pub(crate) fn choices<T: JsonSchema>() -> Vec<(String, String)> {
    let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap_or_default();
    let listed = schema
        .get("oneOf")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    listed
        .iter()
        .filter_map(|choice| {
            let value = choice
                .get("const")
                .or_else(|| choice.get("required").and_then(|required| required.get(0)))
                .and_then(Value::as_str)?;
            let described = choice
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            Some((value.to_owned(), described))
        })
        .collect()
}

/// `text` read as the value of `T` the config writes so.
pub(crate) fn parsed<T: DeserializeOwned>(text: &str) -> Option<T> {
    T::deserialize(StrDeserializer::<Unread>::new(text)).ok()
}

/// The name the config writes `T`'s default as.
pub(crate) fn default_name<T: JsonSchema + DeserializeOwned + Default + PartialEq>() -> String {
    choices::<T>()
        .into_iter()
        .map(|(name, _)| name)
        .find(|name| parsed::<T>(name).is_some_and(|value| value == T::default()))
        .unwrap_or_default()
}

/// Where the answers come from and the questions go.
pub(crate) struct Asking<'a> {
    /// The operator's answers, a line each.
    pub(crate) answers: &'a mut (dyn AsyncBufRead + Send + Unpin),
    /// Where each question is printed.
    pub(crate) out: &'a mut dyn Write,
    /// Whether each question no flag answers takes its default unasked.
    pub(crate) yes: bool,
}

impl Asking<'_> {
    /// `line` said to the operator.
    pub(crate) fn say(&mut self, line: &str) {
        say(self.out, line);
    }

    /// The next line of the answers, trimmed, or none where they ended.
    async fn line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.answers.read_line(&mut line).await {
            Ok(read) if read > 0 => Some(line.trim().to_owned()),
            _ => None,
        }
    }

    /// The answer to `question`, as `check` reads it.
    pub(crate) async fn answer<T>(
        &mut self,
        question: Question<'_>,
        check: impl Fn(&str) -> Result<T, String>,
    ) -> Result<T, String> {
        let unasked = match (question.given, self.yes) {
            (Some(given), _) => Some(given.trim().to_owned()),
            (None, true) => Some(question.default.clone().ok_or_else(|| {
                format!(
                    "{} has no default; give --{}",
                    question.asked, question.flag
                )
            })?),
            (None, false) => None,
        };
        if let Some(unasked) = unasked {
            return check(&unasked).map_err(|why| format!("--{}: {why}", question.flag));
        }
        let shown = question
            .default
            .as_deref()
            .map_or_else(String::new, |default| format!(" [{default}]"));
        for _ in 0..MOST_REFUSED {
            let _ = write!(self.out, "{}{shown}: ", question.asked);
            let _ = self.out.flush();
            let line = self.line().await.ok_or(UNANSWERED)?;
            let answer = match (line.is_empty(), &question.default) {
                (false, _) => Some(line),
                (true, default) => default.clone(),
            };
            let why = match answer.as_deref().map(&check) {
                Some(Ok(checked)) => return Ok(checked),
                Some(Err(why)) => why,
                None => NEEDED.to_owned(),
            };
            self.say(&why);
        }
        Err(format!(
            "no answer to \"{}\" was taken in {MOST_REFUSED} tries; nothing was written",
            question.asked
        ))
    }

    /// The value of `T` `question` chooses, each choice said on its own
    /// line where the operator is asked.
    pub(crate) async fn choice<T: JsonSchema + DeserializeOwned>(
        &mut self,
        question: Question<'_>,
        offered: &[(String, String)],
    ) -> Result<(String, T), String> {
        if question.given.is_none() && !self.yes {
            for (name, described) in offered {
                self.say(&format!("  {name}: {described}"));
            }
        }
        let names = offered
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        let refused = format!("answer one of {}", names.join(", "));
        self.answer(question, |answer| {
            names
                .contains(&answer)
                .then(|| parsed::<T>(answer).map(|value| (answer.to_owned(), value)))
                .flatten()
                .ok_or_else(|| refused.clone())
        })
        .await
    }

    /// The names `question` lists: its flags' where given, else a line
    /// each, ended by an empty line; `none` for none, and an empty first
    /// line for the default.
    pub(crate) async fn list(
        &mut self,
        question: Question<'_>,
        given: &[String],
        default: &[String],
    ) -> Result<Vec<String>, String> {
        if matches!(given, [only] if only.trim() == NONE) {
            return Ok(Vec::new());
        }
        if !given.is_empty() {
            return Ok(given.to_vec());
        }
        if self.yes {
            return Ok(default.to_vec());
        }
        let shown = if default.is_empty() {
            NONE.to_owned()
        } else {
            default.join(", ")
        };
        let _ = write!(
            self.out,
            "{}, one per line, an empty line to end, \"{NONE}\" for none [{shown}]: ",
            question.asked
        );
        let _ = self.out.flush();
        let first = self.line().await.ok_or(UNANSWERED)?;
        match first.as_str() {
            "" => return Ok(default.to_vec()),
            NONE => return Ok(Vec::new()),
            _ => {}
        }
        let mut names = vec![first];
        while let Some(line) = self.line().await.filter(|line| !line.is_empty()) {
            names.push(line);
        }
        Ok(names)
    }

    /// Whether the operator confirms `asked`, `--yes` confirming unasked.
    pub(crate) async fn confirmed(&mut self, asked: &str) -> bool {
        if self.yes {
            return true;
        }
        let _ = write!(self.out, "{asked} [y/N] ");
        let _ = self.out.flush();
        let answer = self.line().await.unwrap_or_default();
        crate::peers::YES.contains(&answer.to_ascii_lowercase().as_str())
    }
}
