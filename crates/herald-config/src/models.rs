//! `[models.*]` and `[tasks.*]` as the file writes them, and as a config
//! holds them (ADR-0057).

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use herald_core::{Options, SecretRef};
use herald_models::Speaks;
use schemars::JsonSchema;
use serde::Deserialize;
use toml::Spanned;

/// A model a task may ask: how it is spoken to and where, which id it
/// answers as, its key, and whether it may be given private data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelConfig {
    /// Its kind, as the config names it: `typesafe`, `openai-compatible`,
    /// or a preset's name.
    pub kind: String,
    /// How it is spoken to.
    pub speaks: Speaks,
    /// Its API's base.
    pub base: String,
    /// The model's id.
    pub model: String,
    /// Its key, where it takes one.
    pub key: Option<SecretRef>,
    /// The header its key goes in, where not `Authorization: Bearer`.
    pub header: Option<String>,
    /// Whether it may be given a private or internal repository's data.
    pub private: bool,
    /// Whether it is reached over plain HTTP at an address not this
    /// machine's.
    pub insecure: bool,
    /// The most bytes of its answer read.
    pub most: usize,
}

/// A task: one closed-set question, the models asked it in turn, and how
/// sure an answer must be.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    /// The models asked, in turn, until one answers.
    pub models: Vec<String>,
    /// What to decide.
    pub instructions: String,
    /// The options it may be answered with.
    pub options: Options,
    /// The least probability an answer is taken at, where a model gives
    /// one; any answer among the options where absent.
    pub least: Option<f64>,
    /// The most pull requests it is asked of a round.
    pub most: usize,
}

/// The most pull requests a task is asked of a round where it names no
/// `most`.
pub const MOST_ASKED: usize = 50;

/// `[models.<name>]`: a model a task may ask.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Model")]
pub(crate) struct ModelFile {
    /// How it is spoken to: "typesafe" for TypeSafe's Jev,
    /// "openai-compatible" for any server speaking OpenAI's chat
    /// completions, or a preset of it, such as "ollama" or "mistral".
    #[schemars(with = "String", example = &"ollama")]
    pub(crate) kind: Spanned<String>,
    /// The model's id, pinned rather than an alias such as `jev-latest`.
    #[schemars(with = "String", example = &"jev-1.14.0")]
    pub(crate) model: Spanned<String>,
    /// Its API's base, such as `http://127.0.0.1:11434/v1`; the preset's
    /// where absent, and required where it has none.
    #[schemars(with = "Option<String>")]
    pub(crate) base: Option<Spanned<String>>,
    /// Its key, where it takes one.
    pub(crate) key: Option<SecretRef>,
    /// The header its key goes in, such as "api-key"; `Authorization:
    /// Bearer` where absent and the preset names none.
    pub(crate) header: Option<String>,
    /// Whether it may be given a private or internal repository's data,
    /// which no model is, on this machine or elsewhere, unless it says
    /// `private = true`.
    #[serde(default)]
    pub(crate) private: bool,
    /// Whether it may be reached over plain HTTP at an address that is not
    /// this machine's, which doctor names.
    #[serde(default)]
    pub(crate) insecure: bool,
    /// The most bytes of its answer read: 65536 where absent.
    pub(crate) most: Option<NonZeroUsize>,
}

/// `[tasks.<name>]`: one closed-set question, asked of its models in
/// turn where a view's rules cannot tell.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Task")]
pub(crate) struct TaskFile {
    /// The models asked, in turn, until one answers, such as
    /// `["jev", "local"]`.
    #[schemars(with = "Vec<String>", length(min = 1))]
    pub(crate) models: Spanned<Vec<String>>,
    /// What to decide, in a sentence.
    pub(crate) instructions: String,
    /// The options it may be answered with, each a key and what it means.
    #[schemars(with = "BTreeMap<String, String>")]
    pub(crate) options: Spanned<BTreeMap<String, String>>,
    /// The least probability an answer is taken at, from 0 to 1, where a
    /// model gives one; where a model gives none, no answer is taken.
    #[schemars(with = "Option<f64>", range(min = 0.0, max = 1.0))]
    pub(crate) least: Option<Spanned<f64>>,
    /// The most pull requests it is asked of a round: 50 where absent;
    /// the rest read as unknown.
    pub(crate) most: Option<NonZeroUsize>,
}
