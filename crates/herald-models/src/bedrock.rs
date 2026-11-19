//! Amazon Bedrock's Converse API: the questions in the system prompt, the
//! state as one JSON-encoded string in the user's message, an answer held
//! to a JSON schema through `outputConfig`; where no Bedrock API key is
//! given, its caller signs each request with AWS Signature Version 4
//! (ADR-0059).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use herald_core::{Answer, Question};
use herald_ports::Secret;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Unanswered;
use crate::asked::{chosen, schema, sent, told};

/// The service whose endpoint a region's models are reached at.
pub const ENDPOINT: &str = "bedrock-runtime";

/// The service a request's signature is scoped to.
pub const SERVICE: &str = "bedrock";

/// What a request is signed with where no Bedrock API key is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    /// The region, such as `eu-west-1`.
    pub region: String,
    /// The access key's id.
    pub access_key_id: Secret,
    /// The secret access key.
    pub secret_access_key: Secret,
    /// A session token, where the credentials are temporary.
    pub session_token: Option<Secret>,
}

/// Whether `region` is written as AWS writes a region's name: lowercase
/// letters, digits and hyphens, from a letter.
#[must_use]
pub fn region(region: &str) -> bool {
    region.starts_with(|first: char| first.is_ascii_lowercase())
        && region
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// `text` with every byte but an unreserved one percent-encoded, as a
/// path segment and a signature's canonical URI write it.
fn encoded(text: &str) -> String {
    text.bytes().fold(String::new(), |mut written, byte| {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            written.push(char::from(byte));
        } else {
            let _ = write!(written, "%{byte:02X}");
        }
        written
    })
}

/// Where the model `segment` names is asked, below the base.
fn at(segment: &str) -> String {
    format!("/model/{segment}/converse")
}

/// Where `model` is asked, below the base.
pub(crate) fn path(model: &str) -> String {
    at(&encoded(model))
}

/// Where `model` is asked, below the base, as a signature's canonical URI
/// writes it: each segment encoded once more.
#[must_use]
pub fn canonical(model: &str) -> String {
    at(&encoded(&encoded(model)))
}

/// The body asking `questions` of `state`, answered within `tokens`.
pub(crate) fn body(tokens: u32, state: &str, questions: &[Question<'_>]) -> Value {
    json!({
        "system": [{ "text": told(questions) }],
        "messages": [{ "role": "user", "content": [{ "text": sent(state) }] }],
        "inferenceConfig": { "maxTokens": tokens },
        "outputConfig": {
            "textFormat": {
                "type": "json_schema",
                "structure": {
                    "jsonSchema": { "name": "decisions", "schema": schema(questions).to_string() },
                },
            },
        },
    })
}

#[derive(Deserialize)]
struct Conversed {
    output: Option<Output>,
    trace: Option<Trace>,
}

#[derive(Deserialize)]
struct Output {
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    content: Vec<Block>,
}

#[derive(Deserialize)]
struct Block {
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Trace {
    prompt_router: Option<Routed>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Routed {
    invoked_model_id: String,
}

/// The answer `body` holds to each of `ids`, read within `most` bytes: the
/// first text block, read as JSON, answered as the model a prompt router
/// chose, or else as `model`.
pub(crate) fn answers(
    most: usize,
    (body, model): (&[u8], &str),
    ids: &[&str],
) -> Result<BTreeMap<String, Answer>, Unanswered> {
    let conversed: Conversed = crate::read(most, body)?;
    let text = conversed
        .output
        .and_then(|output| output.message)
        .and_then(|message| message.content.into_iter().find_map(|block| block.text))
        .unwrap_or_default();
    let answered = conversed
        .trace
        .and_then(|trace| trace.prompt_router)
        .map_or_else(|| model.to_owned(), |routed| routed.invoked_model_id);
    chosen(most, &answered, &text, ids)
}
