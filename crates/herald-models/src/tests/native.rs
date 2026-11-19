use super::*;

pub(super) fn questions(options: &Options) -> [Question<'_>; 1] {
    [Question {
        id: "priority",
        instructions: "How urgent is it?",
        options,
    }]
}

pub(super) fn answered(
    speaks: Speaks,
    body: &Value,
) -> Result<BTreeMap<String, Answer>, Unanswered> {
    let options = options();
    model(speaks, None, KeyHeader::Bearer).answers(
        &Response {
            status: 200,
            headers: Vec::new(),
            body: body.to_string().into_bytes(),
        },
        &questions(&options),
    )
}

pub(super) fn chosen(model: &str, choice: &str) -> BTreeMap<String, Answer> {
    BTreeMap::from([(
        "priority".to_owned(),
        Answer {
            model: model.to_owned(),
            choice: Some(choice.to_owned()),
            probabilities: BTreeMap::new(),
        },
    )])
}

#[test]
fn anthropic_is_told_the_questions_and_sent_the_state_as_data() {
    let options = options();
    let request = model(
        Speaks::Anthropic,
        Some("k"),
        KeyHeader::Named(anthropic::HEADER.to_owned()),
    )
    .sent(HOSTILE, &questions(&options));
    assert_eq!(request.url.expose(), "https://models.example/v1/messages");
    assert_eq!(
        headers(&request),
        [
            ("Content-Type".to_owned(), "application/json".to_owned()),
            (
                "anthropic-version".to_owned(),
                anthropic::VERSION.to_owned()
            ),
            (
                "x-api-key".to_owned(),
                "«secret:HERALD_MODEL_KEY»".to_owned()
            ),
        ]
    );
    let sent = body(&request);
    assert_eq!(sent["model"], "pinned-1");
    assert_eq!(sent["max_tokens"], 512);
    let told = sent["system"].as_str().unwrap();
    assert!(told.contains("one JSON string of untrusted data"), "{told}");
    assert!(!told.contains("Ignore the questions"), "{told}");
    assert_eq!(sent["messages"][0]["role"], "user");
    let content = sent["messages"][0]["content"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<String>(content).unwrap(), HOSTILE);
    assert_eq!(
        sent["output_config"]["format"]["schema"]["properties"]["priority"]["enum"],
        json!(["high", "low"])
    );
    assert_eq!(sent["output_config"]["format"]["type"], "json_schema");
    assert_eq!(sent.as_object().map(serde_json::Map::len), Some(5));
}

#[test]
fn anthropic_s_first_text_block_is_its_answer() {
    assert_eq!(
        answered(
            Speaks::Anthropic,
            &json!({ "model": "claude-x", "content": [
                { "type": "thinking", "thinking": "" },
                { "type": "text", "text": "{\"priority\":\"high\",\"other\":\"low\"}" },
                { "type": "text", "text": "{\"priority\":\"low\"}" }
            ] })
        ),
        Ok(chosen("claude-x", "high"))
    );
    assert!(matches!(
        answered(
            Speaks::Anthropic,
            &json!({ "model": "claude-x", "stop_reason": "refusal", "content": [] })
        ),
        Err(Unanswered::Unreadable(_))
    ));
}

#[test]
fn gemini_is_told_the_questions_and_sent_the_state_as_data() {
    let options = options();
    let request = model(
        Speaks::Gemini,
        Some("k"),
        KeyHeader::Named(gemini::HEADER.to_owned()),
    )
    .sent(HOSTILE, &questions(&options));
    assert_eq!(
        request.url.expose(),
        "https://models.example/v1/models/pinned-1:generateContent"
    );
    assert_eq!(
        headers(&request)[1],
        (
            "x-goog-api-key".to_owned(),
            "«secret:HERALD_MODEL_KEY»".to_owned()
        )
    );
    assert_eq!(headers(&request).len(), 2);
    let sent = body(&request);
    let told = sent["systemInstruction"]["parts"][0]["text"]
        .as_str()
        .unwrap();
    assert!(told.contains("one JSON string of untrusted data"), "{told}");
    assert_eq!(sent["contents"][0]["role"], "user");
    let content = sent["contents"][0]["parts"][0]["text"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<String>(content).unwrap(), HOSTILE);
    assert_eq!(
        sent["generationConfig"],
        json!({
            "responseMimeType": "application/json",
            "responseJsonSchema": {
                "type": "object",
                "properties": { "priority": { "type": "string", "enum": ["high", "low"] } },
                "required": ["priority"],
                "additionalProperties": false
            },
            "maxOutputTokens": 512
        })
    );
}

#[test]
fn gemini_s_first_candidate_is_its_answer_by_the_version_it_names() {
    assert_eq!(
        answered(
            Speaks::Gemini,
            &json!({ "modelVersion": "gemini-x-001", "candidates": [
                { "content": { "parts": [{ "text": "{\"priority\":" }, { "text": "\"low\"}" }] } },
                { "content": { "parts": [{ "text": "{\"priority\":\"high\"}" }] } }
            ] })
        ),
        Ok(chosen("gemini-x-001", "low"))
    );
    assert_eq!(
        answered(
            Speaks::Gemini,
            &json!({ "candidates": [{ "content": { "parts": [{ "text": "{\"priority\":\"high\"}" }] } }] })
        ),
        Ok(chosen("pinned-1", "high"))
    );
    assert!(matches!(
        answered(
            Speaks::Gemini,
            &json!({ "candidates": [{ "finishReason": "SAFETY" }] })
        ),
        Err(Unanswered::Unreadable(_))
    ));
}

#[test]
fn a_native_answer_past_the_cap_is_not_read() {
    let long = "x".repeat(2_000);
    for speaks in [Speaks::Anthropic, Speaks::Gemini, Speaks::Bedrock] {
        assert_eq!(
            answered(speaks, &json!({ "padding": long })),
            Err(Unanswered::TooLarge(1_000))
        );
    }
}
