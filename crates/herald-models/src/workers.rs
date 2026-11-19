//! A model Cloudflare Workers AI runs, through the Worker's binding: the
//! questions in the system message, the state as one JSON-encoded string
//! in the user's, and an answer held to a JSON schema through
//! `response_format` (ADR-0059).

use std::collections::BTreeMap;

use herald_core::{Answer, Question};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Unanswered;
use crate::asked::{chosen, schema, sent, told};

/// The inputs asking `questions` of `state`, answered within `tokens`.
pub(crate) fn inputs(tokens: u32, state: &str, questions: &[Question<'_>]) -> Vec<u8> {
    json!({
        "messages": [
            { "role": "system", "content": told(questions) },
            { "role": "user", "content": sent(state) },
        ],
        "response_format": { "type": "json_schema", "json_schema": schema(questions) },
        "max_tokens": tokens,
    })
    .to_string()
    .into_bytes()
}

#[derive(Deserialize)]
struct Ran {
    response: Option<Value>,
}

/// The answer `body` holds to each of `ids`, read within `most` bytes, as
/// `model`: its response, the JSON object itself or a text holding it.
pub(crate) fn answers(
    most: usize,
    (body, model): (&[u8], &str),
    ids: &[&str],
) -> Result<BTreeMap<String, Answer>, Unanswered> {
    let ran: Ran = crate::read(most, body)?;
    let text = match ran.response {
        Some(Value::String(text)) => text,
        Some(object @ Value::Object(_)) => object.to_string(),
        Some(_) | None => String::new(),
    };
    chosen(most, model, &text, ids)
}
