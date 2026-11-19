//! Anthropic's Messages API: the questions in the system prompt, the state
//! as one JSON-encoded string in the user's message, and an answer held to
//! a JSON schema through `output_config` (ADR-0059).

use std::collections::BTreeMap;

use herald_core::{Answer, Question};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Unanswered;
use crate::asked::{chosen, schema, sent, told};

/// Where the API is asked, below its base.
pub(crate) const PATH: &str = "/messages";

/// The base Anthropic's API is reached at where a config names none.
pub const BASE: &str = "https://api.anthropic.com/v1";

/// The header its key goes in.
pub const HEADER: &str = "x-api-key";

/// The header naming the API's version.
pub(crate) const VERSION_HEADER: &str = "anthropic-version";

/// The API's version herald speaks.
pub const VERSION: &str = "2026-10-21";

/// The body asking `questions` of `state` of `model`, answered within
/// `tokens`.
pub(crate) fn body(model: &str, tokens: u32, state: &str, questions: &[Question<'_>]) -> Value {
    json!({
        "model": model,
        "max_tokens": tokens,
        "system": told(questions),
        "messages": [{ "role": "user", "content": sent(state) }],
        "output_config": {
            "format": { "type": "json_schema", "schema": schema(questions) },
        },
    })
}

#[derive(Deserialize)]
struct Message {
    model: String,
    #[serde(default)]
    content: Vec<Block>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

/// The answer `body` holds to each of `ids`, read within `most` bytes: the
/// first text block, read as JSON.
pub(crate) fn answers(
    most: usize,
    body: &[u8],
    ids: &[&str],
) -> Result<BTreeMap<String, Answer>, Unanswered> {
    let message: Message = crate::read(most, body)?;
    let text = message
        .content
        .into_iter()
        .find(|block| block.kind == "text")
        .map(|block| block.text)
        .unwrap_or_default();
    chosen(most, &message.model, &text, ids)
}
