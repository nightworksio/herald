use super::native::{answered, chosen, questions};
use super::*;

const ROUTED: &str = "arn:aws:bedrock:eu-west-1:123456789012:application-inference-profile/a:1";

fn keys(token: bool) -> bedrock::Keys {
    bedrock::Keys {
        region: "eu-west-1".to_owned(),
        access_key_id: Secret::new("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE"),
        secret_access_key: Secret::new("AWS_SECRET_ACCESS_KEY", "secret"),
        session_token: token.then(|| Secret::new("AWS_SESSION_TOKEN", "session")),
    }
}

fn bedrock(key: Option<&str>, token: bool) -> Model {
    Model {
        base: "https://bedrock-runtime.eu-west-1.amazonaws.com/".to_owned(),
        model: "anthropic.claude-x-v1:0".to_owned(),
        aws: Some(keys(token)),
        ..model(Speaks::Bedrock, key, KeyHeader::Bearer)
    }
}

#[test]
fn a_model_s_requests_are_left_to_sign_only_on_bedrock_with_no_api_key() {
    assert_eq!(bedrock(None, true).signed_by(), Some(&keys(true)));
    assert_eq!(bedrock(Some("k"), true).signed_by(), None);
    let anthropic = Model {
        speaks: Speaks::Anthropic,
        ..bedrock(None, true)
    };
    assert_eq!(anthropic.signed_by(), None);
}

#[test]
fn bedrock_is_told_the_questions_and_sent_the_state_as_data() {
    let options = options();
    let request = bedrock(None, false).sent(HOSTILE, &questions(&options));
    assert_eq!(
        request.url.expose(),
        "https://bedrock-runtime.eu-west-1.amazonaws.com/model/anthropic.claude-x-v1%3A0/converse"
    );
    let sent = body(&request);
    let told = sent["system"][0]["text"].as_str().unwrap();
    assert!(told.contains("one JSON string of untrusted data"), "{told}");
    assert!(!told.contains("Ignore the questions"), "{told}");
    assert_eq!(sent["messages"][0]["role"], "user");
    let content = sent["messages"][0]["content"][0]["text"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<String>(content).unwrap(), HOSTILE);
    assert_eq!(sent["inferenceConfig"], json!({ "maxTokens": 512 }));
    let format = &sent["outputConfig"]["textFormat"];
    assert_eq!(format["type"], "json_schema");
    assert_eq!(format["structure"]["jsonSchema"]["name"], "decisions");
    let schema: Value = serde_json::from_str(
        format["structure"]["jsonSchema"]["schema"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        schema["properties"]["priority"]["enum"],
        json!(["high", "low"])
    );
    assert_eq!(sent.as_object().map(serde_json::Map::len), Some(4));
}

#[test]
fn a_request_to_sign_names_no_header_and_its_canonical_path_is_encoded_twice() {
    let options = options();
    let request = bedrock(None, true).sent(HOSTILE, &questions(&options));
    assert_eq!(headers(&request), Vec::new());
    assert_eq!(request.most, 1_000);
    assert_eq!(
        bedrock::canonical("anthropic.claude-x-v1:0"),
        "/model/anthropic.claude-x-v1%253A0/converse"
    );
}

#[test]
fn a_bedrock_api_key_is_sent_as_a_bearer_and_nothing_is_signed() {
    let options = options();
    let request = bedrock(Some("k"), true).sent(HOSTILE, &questions(&options));
    assert_eq!(
        headers(&request),
        [
            ("Content-Type".to_owned(), "application/json".to_owned()),
            (
                "Authorization".to_owned(),
                "Bearer «secret:HERALD_MODEL_KEY»".to_owned()
            ),
        ]
    );
}

#[test]
fn every_byte_of_a_model_s_id_but_an_unreserved_one_is_encoded() {
    let options = options();
    let model = Model {
        model: ROUTED.to_owned(),
        ..bedrock(Some("k"), false)
    };
    assert_eq!(
        model.sent("", &questions(&options)).url.expose(),
        "https://bedrock-runtime.eu-west-1.amazonaws.com/model/arn%3Aaws%3Abedrock%3Aeu-west-1%3A123456789012%3Aapplication-inference-profile%2Fa%3A1/converse"
    );
}

#[test]
fn bedrock_s_first_text_block_is_its_answer_by_the_model_a_router_chose() {
    assert_eq!(
        answered(
            Speaks::Bedrock,
            &json!({ "output": { "message": { "role": "assistant", "content": [
                { "reasoningContent": {} },
                { "text": "{\"priority\":\"high\",\"other\":\"low\"}" },
                { "text": "{\"priority\":\"low\"}" }
            ] } }, "stopReason": "end_turn" })
        ),
        Ok(chosen("pinned-1", "high"))
    );
    assert_eq!(
        answered(
            Speaks::Bedrock,
            &json!({
                "output": { "message": { "content": [{ "text": "{\"priority\":\"low\"}" }] } },
                "trace": { "promptRouter": { "invokedModelId": ROUTED } }
            })
        ),
        Ok(chosen(ROUTED, "low"))
    );
    for unanswered in [
        json!({ "output": {}, "trace": {} }),
        json!({ "output": { "message": {} } }),
        json!({ "stopReason": "guardrail_intervened" }),
    ] {
        assert!(matches!(
            answered(Speaks::Bedrock, &unanswered),
            Err(Unanswered::Unreadable(_))
        ));
    }
}

#[test]
fn a_region_is_written_as_aws_writes_its_name() {
    for region in ["eu-west-1", "us-gov-west-1", "cn-north-1", "eusc-de-east-1"] {
        assert!(bedrock::region(region), "{region}");
    }
    for region in [
        "",
        "EU-west-1",
        "1-west",
        "eu/west",
        "eu_west",
        "eu-west-1.",
    ] {
        assert!(!bedrock::region(region), "{region}");
    }
}
