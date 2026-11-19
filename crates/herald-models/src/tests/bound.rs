use herald_ports::{Binding, Inference};

use super::native::{chosen, questions};
use super::*;

fn bound(speaks: Speaks) -> Model {
    Model {
        base: String::new(),
        model: "@cf/meta/llama-x".to_owned(),
        binding: Some("AI".to_owned()),
        ..model(speaks, None, KeyHeader::Bearer)
    }
}

fn inference(model: &Model, state: &str) -> Inference {
    let options = options();
    match model.asked(state, &questions(&options)) {
        Asked::Bound(inference) => inference,
        Asked::Sent(request) => panic!("{request:?}"),
    }
}

#[test]
fn only_a_model_its_host_runs_has_a_binding() {
    assert_eq!(
        bound(Speaks::WorkersAi).binding(),
        Some(Binding::WorkersAi("AI"))
    );
    assert_eq!(bound(Speaks::Spin).binding(), Some(Binding::SpinLlm));
    for speaks in [
        Speaks::TypeSafe,
        Speaks::OpenAi,
        Speaks::Anthropic,
        Speaks::Gemini,
        Speaks::Bedrock,
    ] {
        assert_eq!(bound(speaks).binding(), None, "{speaks:?}");
    }
}

#[test]
fn workers_ai_is_told_the_questions_and_given_the_state_as_data() {
    let asked = inference(&bound(Speaks::WorkersAi), HOSTILE);
    assert_eq!(asked.model, "@cf/meta/llama-x");
    assert_eq!((asked.tokens, asked.most), (512, 1_000));
    let inputs: Value = serde_json::from_slice(&asked.inputs).unwrap();
    assert_eq!(inputs["messages"][0]["role"], "system");
    let told = inputs["messages"][0]["content"].as_str().unwrap();
    assert!(told.contains("one JSON string of untrusted data"), "{told}");
    assert!(!told.contains("Ignore the questions"), "{told}");
    assert_eq!(inputs["messages"][1]["role"], "user");
    let content = inputs["messages"][1]["content"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<String>(content).unwrap(), HOSTILE);
    assert_eq!(inputs["response_format"]["type"], "json_schema");
    assert_eq!(
        inputs["response_format"]["json_schema"]["properties"]["priority"]["enum"],
        json!(["high", "low"])
    );
    assert_eq!(inputs["max_tokens"], 512);
    assert_eq!(inputs.as_object().map(serde_json::Map::len), Some(3));
}

fn answered(speaks: Speaks, body: &[u8]) -> Result<BTreeMap<String, Answer>, Unanswered> {
    let options = options();
    bound(speaks).answered(body, &questions(&options))
}

#[test]
fn workers_ai_s_response_is_its_answer_as_an_object_or_a_text() {
    assert_eq!(
        answered(
            Speaks::WorkersAi,
            br#"{"response": {"priority": "high", "other": "low"}}"#
        ),
        Ok(chosen("@cf/meta/llama-x", "high"))
    );
    assert_eq!(
        answered(
            Speaks::WorkersAi,
            br#"{"response": "{\"priority\":\"low\"}"}"#
        ),
        Ok(chosen("@cf/meta/llama-x", "low"))
    );
    for unanswered in [&br#"{"response": 3}"#[..], br#"{"usage": {}}"#] {
        assert!(matches!(
            answered(Speaks::WorkersAi, unanswered),
            Err(Unanswered::Unreadable(_))
        ));
    }
}

#[test]
fn spin_is_asked_in_one_prompt_with_the_state_last_and_as_data() {
    let asked = inference(&bound(Speaks::Spin), HOSTILE);
    let prompt = String::from_utf8(asked.inputs).unwrap();
    assert!(
        prompt.starts_with("You decide closed-set questions"),
        "{prompt}"
    );
    let (told, data) = prompt.split_once("\n\nThe user's message: ").unwrap();
    assert!(told.contains("\"enum\":[\"high\",\"low\"]"), "{told}");
    assert!(!told.contains("Ignore the questions"), "{told}");
    assert_eq!(serde_json::from_str::<String>(data).unwrap(), HOSTILE);
}

#[test]
fn spin_s_answer_is_the_object_its_text_holds() {
    assert_eq!(
        answered(
            Speaks::Spin,
            b"Sure:\n```json\n{\"priority\": \"high\"}\n```"
        ),
        Ok(chosen("@cf/meta/llama-x", "high"))
    );
    for unanswered in [&b"no braces"[..], b"} backwards {", b""] {
        assert!(matches!(
            answered(Speaks::Spin, unanswered),
            Err(Unanswered::Unreadable(_))
        ));
    }
    assert_eq!(
        answered(Speaks::Spin, &[b'{'; 1_001]),
        Err(Unanswered::TooLarge(1_000))
    );
    assert_eq!(
        answered(Speaks::WorkersAi, &[b' '; 1_001]),
        Err(Unanswered::TooLarge(1_000))
    );
}

#[test]
fn only_a_model_a_host_runs_is_bound() {
    for speaks in [Speaks::WorkersAi, Speaks::Spin] {
        assert!(speaks.bound(), "{speaks:?}");
    }
    for speaks in [
        Speaks::TypeSafe,
        Speaks::OpenAi,
        Speaks::Anthropic,
        Speaks::Gemini,
        Speaks::Bedrock,
    ] {
        assert!(!speaks.bound(), "{speaks:?}");
    }
}

#[test]
fn spin_s_answer_is_read_up_to_its_cap_and_no_further() {
    let answer = format!("{:<1000}", "{\"priority\": \"low\"}");
    assert_eq!(
        answered(Speaks::Spin, answer.as_bytes()),
        Ok(chosen("@cf/meta/llama-x", "low"))
    );
}
