//! OpenAI's chat completions, which most servers speak: the questions in
//! the system message, the state as one JSON-encoded string in the user's,
//! which no text it holds can end, and an answer held to a JSON schema
//! whose every field is one of its question's options (ADR-0057).

use std::collections::BTreeMap;

use herald_core::{Answer, Question};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Unanswered;
use crate::asked::{chosen, schema, sent, told};

/// Where the API is asked, below its base.
pub(crate) const PATH: &str = "/chat/completions";

/// The body asking `questions` of `state` of `model`.
pub(crate) fn body(model: &str, state: &str, questions: &[Question<'_>]) -> Value {
    json!({
        "model": model,
        "messages": [
            { "role": "system", "content": told(questions) },
            { "role": "user", "content": sent(state) },
        ],
        "response_format": {
            "type": "json_schema",
            "json_schema": { "name": "decisions", "strict": true, "schema": schema(questions) },
        },
    })
}

#[derive(Deserialize)]
struct Completion {
    model: String,
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: Option<String>,
}

/// The answer `body` holds to each of `ids`, read within `most` bytes:
/// the first choice's content, read as JSON.
pub(crate) fn answers(
    most: usize,
    body: &[u8],
    ids: &[&str],
) -> Result<BTreeMap<String, Answer>, Unanswered> {
    let completion: Completion = crate::read(most, body)?;
    let content = completion
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .unwrap_or_default();
    chosen(most, &completion.model, &content, ids)
}
