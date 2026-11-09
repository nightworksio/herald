use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, de};

/// The characters a destination writes where its markup has none: a task
/// done and to do, a rule (drawn three wide), and a list's bullet where a
/// list is written as text, in Slack and in plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Symbols {
    /// A task done; `☑` where unset.
    #[serde(default = "done", deserialize_with = "one")]
    #[schemars(with = "Option<OneCharacter>")]
    pub done: char,
    /// A task to do; `☐` where unset.
    #[serde(default = "todo", deserialize_with = "one")]
    #[schemars(with = "Option<OneCharacter>")]
    pub todo: char,
    /// A rule's character, drawn three wide; `─` where unset.
    #[serde(default = "rule", deserialize_with = "one")]
    #[schemars(with = "Option<OneCharacter>")]
    pub rule: char,
    /// A list's bullet where a list is written as text: `•` in Slack and
    /// `-` in plain text where unset.
    #[serde(default, deserialize_with = "maybe_one")]
    #[schemars(with = "Option<OneCharacter>")]
    pub bullet: Option<char>,
}

impl Symbols {
    /// The characters where a config sets none.
    pub const DEFAULT: Self = Self {
        done: '☑',
        todo: '☐',
        rule: '─',
        bullet: None,
    };
}

impl Default for Symbols {
    fn default() -> Self {
        Self::DEFAULT
    }
}

const fn done() -> char {
    Symbols::DEFAULT.done
}

const fn todo() -> char {
    Symbols::DEFAULT.todo
}

const fn rule() -> char {
    Symbols::DEFAULT.rule
}

/// One character, as a config writes it.
#[derive(JsonSchema)]
#[schemars(transparent)]
#[allow(dead_code, reason = "read only by the schema's derive")]
struct OneCharacter(#[schemars(length(equal = 1))] String);

struct Written(String);

impl fmt::Display for Written {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\"{}\" is not one character; give one, such as \"•\"",
            self.0
        )
    }
}

fn single<E: de::Error>(text: String) -> Result<char, E> {
    let mut characters = text.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) if !character.is_control() => Ok(character),
        _ => Err(E::custom(Written(text))),
    }
}

fn one<'de, D: Deserializer<'de>>(deserializer: D) -> Result<char, D::Error> {
    single(String::deserialize(deserializer)?)
}

fn maybe_one<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<char>, D::Error> {
    single(String::deserialize(deserializer)?).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_symbol_is_one_character_and_today_s_where_unset() {
        let set: Symbols = serde_json::from_str(r#"{"done": "x", "bullet": "*"}"#).unwrap();
        assert_eq!(
            set,
            Symbols {
                done: 'x',
                bullet: Some('*'),
                ..Symbols::DEFAULT
            }
        );
        assert_eq!(
            serde_json::from_str::<Symbols>("{}").unwrap(),
            Symbols::default()
        );
        for refused in [
            r#"{"rule": "--"}"#,
            r#"{"todo": ""}"#,
            r#"{"bullet": "\n"}"#,
        ] {
            let error = serde_json::from_str::<Symbols>(refused)
                .unwrap_err()
                .to_string();
            assert!(error.contains("is not one character"), "{error}");
        }
    }
}
