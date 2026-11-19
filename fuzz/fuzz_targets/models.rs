//! Whatever a model answers, reading it never panics: each kind's answer
//! is read within its cap, and only the questions asked are answered.

#![no_main]

use std::collections::BTreeMap;

use herald_core::{Options, Question};
use herald_models::{Asked, KeyHeader, Model, Speaks, bedrock};
use herald_ports::{Response, Secret};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let options = Options::new(BTreeMap::from([
        ("high".to_owned(), "it blocks a release".to_owned()),
        ("low".to_owned(), "it can wait".to_owned()),
    ]))
    .expect("two options");
    let questions = [Question {
        id: "priority",
        instructions: "How urgent is it?",
        options: &options,
    }];
    let state = String::from_utf8_lossy(bytes);
    for speaks in [
        Speaks::TypeSafe,
        Speaks::OpenAi,
        Speaks::Anthropic,
        Speaks::Gemini,
        Speaks::Bedrock,
        Speaks::WorkersAi,
        Speaks::Spin,
    ] {
        let model = Model {
            speaks,
            base: "https://models.example/v1".to_owned(),
            model: "pinned-1".to_owned(),
            key: None,
            header: KeyHeader::Bearer,
            most: 4_096,
            tokens: 512,
            aws: Some(bedrock::Keys {
                region: "eu-west-1".to_owned(),
                access_key_id: Secret::new("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE"),
                secret_access_key: Secret::new("AWS_SECRET_ACCESS_KEY", "fuzzed"),
                session_token: None,
            }),
            binding: Some("AI".to_owned()),
        };
        let asked = match model.asked(&state, &questions) {
            Asked::Sent(request) => request.body,
            Asked::Bound(inference) => inference.inputs,
        };
        assert!(asked.len() > state.len());
        let response = Response {
            status: 200,
            headers: Vec::new(),
            body: bytes.to_vec(),
        };
        if let Ok(answers) = model.answers(&response, &questions) {
            assert!(answers.keys().all(|id| id == "priority"));
            assert!(bytes.len() <= 4_096);
        }
    }
});
