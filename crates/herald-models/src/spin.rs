//! A model Spin runs, through its `llm` interface, which takes one prompt:
//! the questions, the JSON schema the answer holds to, and the state as one
//! JSON-encoded string after them, marked as the data alone (ADR-0059).

use std::collections::BTreeMap;

use herald_core::{Answer, Question};

use crate::Unanswered;
use crate::asked::{chosen, schema, sent, told};

/// The prompt asking `questions` of `state`.
pub(crate) fn prompt(state: &str, questions: &[Question<'_>]) -> Vec<u8> {
    format!(
        "{}\n\nAnswer with one JSON object alone, holding to this JSON schema: {}\n\nThe user's message: {}",
        told(questions),
        schema(questions),
        sent(state)
    )
    .into_bytes()
}

/// The answer `body`, the text `model` wrote, holds to each of `ids`, read
/// within `most` bytes: the JSON object from its first `{` to its last `}`.
pub(crate) fn answers(
    most: usize,
    (body, model): (&[u8], &str),
    ids: &[&str],
) -> Result<BTreeMap<String, Answer>, Unanswered> {
    if body.len() > most {
        return Err(Unanswered::TooLarge(most));
    }
    let text = String::from_utf8_lossy(body);
    let object = text
        .find('{')
        .zip(text.rfind('}'))
        .and_then(|(start, end)| text.get(start..=end))
        .unwrap_or_default();
    chosen(most, model, object, ids)
}
