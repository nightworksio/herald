use std::sync::{Arc, Mutex};

use herald_ports::{Binding, DynInfer, Infer, InferError, Inference};

use super::*;

const BEDROCK: &str =
    "https://bedrock-runtime.eu-west-1.amazonaws.com/model/anthropic.claude-x-v1%3A0/converse";

fn bound() -> Config {
    load(
        "herald.toml",
        r#"version = 1
[store]
kind = "sqlite"
[sources.open]
kind = "github"
repository = "o/open"
token = { secret = "HERALD_GITHUB_TOKEN" }
[models.spin]
kind = "spin"
model = "llama2-chat"
[models.ai]
kind = "workers-ai"
binding = "AI"
model = "@cf/meta/llama-x"
[models.claude]
kind = "bedrock"
region = "eu-west-1"
model = "anthropic.claude-x-v1:0"
[tasks.urgency]
models = ["spin", "ai", "claude"]
instructions = "How urgent is it?"
options = { high = "it blocks a release", low = "it can wait" }
[views.queue]
deciding = "forge"
decide = { priority = "urgency" }
"#,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

/// A host's model that answers `answer`, or fails where it is none,
/// keeping what it was asked.
#[derive(Debug, Clone)]
struct Runs {
    answer: Option<&'static str>,
    asked: Arc<Mutex<Vec<Inference>>>,
}

impl Infer for Runs {
    async fn infer(&self, inference: Inference) -> Result<Vec<u8>, InferError> {
        self.asked.lock().unwrap().push(inference);
        self.answer
            .map(|answer| answer.as_bytes().to_vec())
            .ok_or_else(|| InferError::failed("down"))
    }
}

/// What is decided of the row once `connected` reads it.
fn decided_of(config: &Config, connected: &Connected<Script>) -> Vec<(Slot, String)> {
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &healthy(),
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let board = pollster::block_on(queue(config, connected, &ports, "queue")).unwrap();
    decided(&board).2
}

/// The models of `config` reached with `secrets` through `script`.
fn connected(config: &Config, secrets: &FakeSecrets, script: &Script) -> Connected<Script> {
    let prepared = pollster::block_on(prepare(config, secrets, &FakeSigner, Uses::Polls)).unwrap();
    prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap()
}

fn signing() -> FakeSecrets {
    FakeSecrets::default()
        .with("HERALD_GITHUB_TOKEN", "ghp_x")
        .with("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE")
        .with("AWS_SECRET_ACCESS_KEY", "secret")
}

#[test]
fn a_model_its_host_runs_is_asked_through_the_binding_it_was_given() {
    let spin = Runs {
        answer: None,
        asked: Arc::default(),
    };
    let ai = Runs {
        answer: Some(r#"{"response": {"urgency": "high"}}"#),
        asked: Arc::default(),
    };
    let mut bound_to = Vec::new();
    let script = Script::answering(&[(GRAPHQL, 200, &page("PUBLIC"))]);
    let config = bound();
    let reached = connected(&config, &signing(), &script)
        .bind(|name, binding| {
            bound_to.push((name.to_owned(), format!("{binding:?}")));
            Ok::<Arc<dyn DynInfer>, ()>(match binding {
                Binding::SpinLlm => Arc::new(spin.clone()),
                Binding::WorkersAi(_) => Arc::new(ai.clone()),
            })
        })
        .unwrap();
    let chosen = decided_of(&config, &reached);
    assert_eq!(chosen, [(Slot::Priority, "high".to_owned())]);
    assert_eq!(
        bound_to,
        [
            ("ai".to_owned(), "WorkersAi(\"AI\")".to_owned()),
            ("spin".to_owned(), "SpinLlm".to_owned())
        ]
    );
    let asked = spin.asked.lock().unwrap();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].model, "llama2-chat");
    let prompt = String::from_utf8(asked[0].inputs.clone()).unwrap();
    assert!(prompt.contains("untrusted data"), "{prompt}");
    assert_eq!(ai.asked.lock().unwrap()[0].model, "@cf/meta/llama-x");
    assert_eq!(script.seen().len(), 1, "Bedrock was not asked");
}

#[test]
fn a_model_its_host_runs_is_not_asked_unbound_and_bedrock_is_asked_signed() {
    let answered = json!({ "output": { "message": { "content": [
        { "text": "{\"urgency\":\"low\"}" }
    ] } } })
    .to_string();
    let script = Script::answering(&[(GRAPHQL, 200, &page("PUBLIC")), (BEDROCK, 200, &answered)]);
    let config = bound();
    let chosen = decided_of(&config, &connected(&config, &signing(), &script));
    assert_eq!(chosen, [(Slot::Priority, "low".to_owned())]);
    let headers = &script.asked()[1];
    let authorization = headers
        .iter()
        .find(|(name, _)| name == "Authorization")
        .map(|(_, value)| value.as_str())
        .unwrap();
    assert!(
        authorization.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20261031/eu-west-1/bedrock/aws4_request"
        ),
        "{authorization}"
    );
    assert!(
        headers
            .iter()
            .all(|(name, _)| name != "X-Amz-Security-Token"),
        "{headers:?}"
    );
    let tokened = Script::answering(&[(GRAPHQL, 200, &page("PUBLIC")), (BEDROCK, 200, &answered)]);
    let reached = connected(
        &config,
        &signing().with("AWS_SESSION_TOKEN", "session"),
        &tokened,
    );
    assert_eq!(decided_of(&config, &reached).len(), 1);
    assert!(
        tokened.asked()[1]
            .iter()
            .any(|(name, value)| name == "X-Amz-Security-Token" && value == "session")
    );
}
