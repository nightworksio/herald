//! TypeSafe's System One API: one state, sent as its own JSON field apart
//! from the questions, any number of `choice` questions, each answered
//! with the option chosen and every option's probability (ADR-0057).

use std::collections::BTreeMap;

use herald_core::{Answer, Question};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::Unanswered;

/// Where the API is asked, below its base.
pub(crate) const PATH: &str = "/systemone";

/// The body asking `questions` of `state` of `model`.
pub(crate) fn body(model: &str, state: &str, questions: &[Question<'_>]) -> Value {
    let asked: Map<String, Value> = questions
        .iter()
        .map(|question| {
            (
                question.id.to_owned(),
                json!({
                    "type": "choice",
                    "instructions": question.instructions,
                    "criteria": question.options.as_map(),
                }),
            )
        })
        .collect();
    json!({ "state": state, "model": model, "questions": asked })
}

#[derive(Deserialize)]
struct Answered {
    model: String,
    #[serde(default)]
    answers: BTreeMap<String, Chosen>,
}

#[derive(Deserialize)]
struct Chosen {
    choice: Option<String>,
    #[serde(default)]
    probabilities: BTreeMap<String, f64>,
}

/// The answer `body` holds to each of `ids`, read within `most` bytes.
pub(crate) fn answers(
    most: usize,
    body: &[u8],
    ids: &[&str],
) -> Result<BTreeMap<String, Answer>, Unanswered> {
    let answered: Answered = crate::read(most, body)?;
    Ok(answered
        .answers
        .into_iter()
        .filter(|(id, _)| ids.contains(&id.as_str()))
        .map(|(id, chosen)| {
            (
                id,
                Answer {
                    model: answered.model.clone(),
                    choice: chosen.choice,
                    probabilities: chosen.probabilities,
                },
            )
        })
        .collect())
}
