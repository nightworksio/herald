//! `[models.*]` and `[tasks.*]` as the file writes them, and as a config
//! holds them (ADR-0057).

use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroUsize};

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
    /// `anthropic`, `gemini`, `bedrock`, `spin`, or a preset's name.
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
    /// The most tokens it answers within, where its API takes a cap.
    pub tokens: u32,
    /// Where it is reached through Amazon Bedrock, its region and what its
    /// requests are signed with.
    pub bedrock: Option<BedrockConfig>,
    /// The Worker's binding it is reached through, where Workers AI runs
    /// it beside herald.
    pub binding: Option<String>,
}

/// A model reached through Amazon Bedrock: its region, and the secrets its
/// requests are signed with where it names no key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedrockConfig {
    /// The region.
    pub region: String,
    /// The secret holding the access key's id.
    pub access_key_id: SecretRef,
    /// The secret holding the secret access key.
    pub secret_access_key: SecretRef,
    /// The secret holding a session token, sent where it is set.
    pub session_token: SecretRef,
}

impl ModelConfig {
    /// Every secret a run asking it cannot ask it without: its key, or else
    /// the access key a request to Amazon Bedrock is signed with; a
    /// session token is read where it is set.
    #[must_use]
    pub fn secret_refs(&self) -> Vec<&SecretRef> {
        match (&self.key, &self.bedrock) {
            (Some(key), _) => vec![key],
            (None, Some(bedrock)) => vec![&bedrock.access_key_id, &bedrock.secret_access_key],
            (None, None) => Vec::new(),
        }
    }
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
    /// completions, or a preset of it, such as "ollama" or "mistral",
    /// "anthropic" for Anthropic's Messages API, "gemini" for Gemini's,
    /// "bedrock" for Amazon Bedrock's Converse API, or "spin" for a model
    /// Spin runs; "workers-ai" with a binding is a model Workers AI runs.
    #[schemars(with = "String", example = &"ollama")]
    pub(crate) kind: Spanned<String>,
    /// The model's id, pinned rather than an alias such as `jev-latest`.
    #[schemars(with = "String", example = &"jev-1.14.0")]
    pub(crate) model: Spanned<String>,
    /// Its API's base, such as `http://127.0.0.1:11434/v1`; the preset's
    /// where absent, and required where it has none. A "bedrock" model's
    /// is its region's endpoint where absent.
    #[schemars(with = "Option<String>")]
    pub(crate) base: Option<Spanned<String>>,
    /// Its key, where it takes one; a "bedrock" model's is a Bedrock API
    /// key, sent in place of a signature.
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
    /// The most tokens it answers within, where its kind takes a cap, as
    /// every kind but "typesafe" and "openai-compatible" does: 4096 where
    /// absent.
    pub(crate) tokens: Option<NonZeroU32>,
    /// The region a "bedrock" model is reached in, such as "eu-west-1", in
    /// any partition, such as "us-gov-west-1" or "cn-north-1".
    #[schemars(example = &"eu-west-1")]
    pub(crate) region: Option<String>,
    /// The secret holding the access key's id a "bedrock" model's requests
    /// are signed with where it names no key; `AWS_ACCESS_KEY_ID` where
    /// absent, as a Lambda's role provides it.
    pub(crate) access_key_id: Option<SecretRef>,
    /// The secret holding the secret access key; `AWS_SECRET_ACCESS_KEY`
    /// where absent.
    pub(crate) secret_access_key: Option<SecretRef>,
    /// The secret holding a session token, sent where it is set;
    /// `AWS_SESSION_TOKEN` where absent.
    pub(crate) session_token: Option<SecretRef>,
    /// The Worker's binding a "workers-ai" model is reached through, such
    /// as "AI", in place of Workers AI's REST API; it takes neither a base
    /// nor a key.
    #[schemars(example = &"AI")]
    pub(crate) binding: Option<String>,
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
