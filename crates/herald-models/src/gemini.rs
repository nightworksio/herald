//! Google's Gemini API: the questions in the system instruction, the state
//! as one JSON-encoded string in the user's part, and an answer held to a
//! JSON schema through `responseJsonSchema` (ADR-0059).

use std::collections::BTreeMap;

use herald_core::{Answer, Question};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Unanswered;
use crate::asked::{chosen, schema, sent, told};

/// The base Gemini's API is reached at where a config names none.
pub const BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

/// The header its key goes in.
pub const HEADER: &str = "x-goog-api-key";

/// Where `model` is asked, below the base.
pub(crate) fn path(model: &str) -> String {
    format!("/models/{model}:generateContent")
}

/// The body asking `questions` of `state`, answered within `tokens`.
pub(crate) fn body(tokens: u32, state: &str, questions: &[Question<'_>]) -> Value {
    json!({
        "systemInstruction": { "parts": [{ "text": told(questions) }] },
        "contents": [{ "role": "user", "parts": [{ "text": sent(state) }] }],
        "generationConfig": {
            "responseMimeType": herald_core::media::JSON,
            "responseJsonSchema": schema(questions),
            "maxOutputTokens": tokens,
        },
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Generated {
    #[serde(default)]
    model_version: String,
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<Content>,
}

#[derive(Deserialize)]
struct Content {
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Deserialize)]
struct Part {
    #[serde(default)]
    text: String,
}

/// The answer `body` holds to each of `ids`, read within `most` bytes: the
/// first candidate's text, read as JSON, answered as `model` where the
/// answer names no version.
pub(crate) fn answers(
    most: usize,
    (body, model): (&[u8], &str),
    ids: &[&str],
) -> Result<BTreeMap<String, Answer>, Unanswered> {
    let generated: Generated = crate::read(most, body)?;
    let text: String = generated
        .candidates
        .into_iter()
        .next()
        .and_then(|candidate| candidate.content)
        .map(|content| content.parts.into_iter().map(|part| part.text).collect())
        .unwrap_or_default();
    let version = if generated.model_version.is_empty() {
        model.to_owned()
    } else {
        generated.model_version
    };
    chosen(most, &version, &text, ids)
}
