//! `[models]`, `[tasks]` and a view's `decide` (ADR-0057): each model read
//! against its kind and base, each task against its models, and every
//! mistake named by its key.

use herald_config::{Config, MOST_ASKED, Uses, load};
use herald_core::{SecretRef, Slot};
use herald_models::{MOST_ANSWER, MOST_TOKENS, Speaks};

const EXAMPLE: &str = r#"version = 1

[store]
kind = "sqlite"

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
"#;

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn with(tables: &str) -> String {
    format!("{EXAMPLE}\n{tables}")
}

fn loaded(text: &str) -> Config {
    load("herald.toml", text, &nothing, &crate::common::kinds()).unwrap()
}

fn errors(text: &str) -> String {
    load("herald.toml", text, &nothing, &crate::common::kinds())
        .unwrap_err()
        .to_string()
}

/// A model's name, how it is spoken to, its base and header, whether it
/// may see private data, and the most tokens it answers within.
type Native<'a> = (&'a str, Speaks, &'a str, Option<&'a str>, bool, u32);

/// A model's name, how it is spoken to, its base and its binding.
type Bound<'a> = (&'a str, Speaks, &'a str, Option<&'a str>);

/// A model's name, how it is spoken to, its base and header, whether it
/// may see private data, whether it is insecure, and its cap.
type Modelled<'a> = (&'a str, Speaks, &'a str, Option<&'a str>, bool, bool, usize);

const MODELS: &str = r#"[models.jev]
kind = "typesafe"
model = "jev-1.14.0"
key = { secret = "HERALD_TYPESAFE_KEY" }

[models.local]
kind = "ollama"
model = "qwen3:8b"

[models.azure]
kind = "azure"
model = "gpt-x"
base = "https://acme.openai.azure.com/openai/v1/"
key = { secret = "HERALD_AZURE_KEY" }
most = 4096

[models.lan]
kind = "openai-compatible"
model = "m"
base = "http://192.168.1.20:8000/v1"
insecure = true
private = true

[models.hosted]
kind = "mistral"
model = "mistral-small-2506"
header = "x-key"

[tasks.urgency]
models = ["jev", "local"]
instructions = "How urgent is this pull request?"
options = { high = "it blocks a release", low = "it can wait" }
least = 0.8
most = 10

[tasks.whose]
models = ["local"]
instructions = "Whose turn is it?"
options = { waiting-on-review = "a reviewer's", waiting-on-author = "its author's" }

[views.review]
decide = { priority = "urgency", turn = "whose" }
"#;

#[test]
fn a_model_is_read_with_its_kind_s_base_and_header_and_private_only_where_it_says_so() {
    let config = loaded(&with(MODELS));
    let read: Vec<Modelled<'_>> = config
        .models
        .iter()
        .map(|(name, model)| {
            (
                name.as_str(),
                model.speaks,
                model.base.as_str(),
                model.header.as_deref(),
                model.private,
                model.insecure,
                model.most,
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (
                "azure",
                Speaks::OpenAi,
                "https://acme.openai.azure.com/openai/v1",
                Some("api-key"),
                false,
                false,
                4096
            ),
            (
                "hosted",
                Speaks::OpenAi,
                "https://api.mistral.ai/v1",
                Some("x-key"),
                false,
                false,
                MOST_ANSWER
            ),
            (
                "jev",
                Speaks::TypeSafe,
                "https://api.typesafe.ai/v1",
                None,
                false,
                false,
                MOST_ANSWER
            ),
            (
                "lan",
                Speaks::OpenAi,
                "http://192.168.1.20:8000/v1",
                None,
                true,
                true,
                MOST_ANSWER
            ),
            (
                "local",
                Speaks::OpenAi,
                "http://127.0.0.1:11434/v1",
                None,
                false,
                false,
                MOST_ANSWER
            ),
        ]
    );
    assert_eq!(config.models["jev"].kind, "typesafe");
    assert_eq!(config.models["jev"].model, "jev-1.14.0");
    assert_eq!(
        config.models["jev"].key,
        Some(SecretRef {
            secret: "HERALD_TYPESAFE_KEY".to_owned()
        })
    );
    let urgency = &config.tasks["urgency"];
    assert_eq!(urgency.models, ["jev", "local"]);
    assert_eq!(urgency.instructions, "How urgent is this pull request?");
    assert!(urgency.options.offers("high"));
    assert_eq!(urgency.least, Some(0.8));
    assert_eq!(urgency.most, 10);
    assert_eq!(config.tasks["whose"].most, MOST_ASKED);
    assert_eq!(config.tasks["whose"].least, None);
    let decide: Vec<(Slot, &str)> = config.views["review"]
        .decide
        .iter()
        .map(|(slot, task)| (*slot, task.as_str()))
        .collect();
    assert_eq!(decide, [(Slot::Turn, "whose"), (Slot::Priority, "urgency")]);
    assert!(config.views["queue"].decide.is_empty());
}

#[test]
fn a_model_s_key_is_read_by_a_run_that_polls_or_serves() {
    let config = loaded(&with(MODELS));
    let polled = config.secrets_used(Uses::Polls);
    assert!(polled.contains("HERALD_TYPESAFE_KEY") && polled.contains("HERALD_AZURE_KEY"));
    assert!(
        config
            .secrets_used(Uses::Serves)
            .contains("HERALD_TYPESAFE_KEY")
    );
    assert!(
        !config
            .secrets_used(Uses::Announces)
            .contains("HERALD_TYPESAFE_KEY")
    );
}

#[test]
fn a_model_or_task_of_the_wrong_shape_is_named() {
    let error = errors(&with(
        "[models.odd]\nkind = 3\nmodel = \"m\"\n\n[tasks.odd]\nmodels = \"jev\"\ninstructions = \"?\"\noptions = { a = \"x\", b = \"y\" }\n",
    ));
    assert!(error.contains("models.odd.kind"), "{error}");
    assert!(error.contains("tasks.odd.models"), "{error}");
}

#[test]
fn every_mistake_in_a_model_task_or_decide_is_named_by_its_key() {
    let error = errors(&with(
        r#"[models.odd]
kind = "oracle"
model = "m"

[models.bare]
kind = "openai-compatible"
model = "m"

[models.plain]
kind = "openai-compatible"
model = "m"
base = "http://10.0.0.2/v1"

[models.ftp]
kind = "openai-compatible"
model = "m"
base = "ftp://10.0.0.2/v1"

[models.fine]
kind = "ollama"
model = "m"

[tasks.none]
models = []
instructions = "?"
options = { a = "x", b = "y" }

[tasks.lost]
models = ["missing"]
instructions = "?"
options = { a = "x" }
least = 2.0

[tasks.state]
models = ["fine"]
instructions = "?"
options = { later = "x", sooner = "y" }

[views.mine]
decide = { turn = "state", noise = "nothing" }
"#,
    ));
    for wanted in [
        "models.odd.kind: there is no model kind \"oracle\"; the kinds are \"typesafe\", \"openai-compatible\", \"anthropic\", \"gemini\", \"bedrock\", \"spin\", \"ollama\"",
        "models.bare.base: a model is reached at a base URL, such as base = \"http://127.0.0.1:11434/v1\"",
        "models.plain.base: \"http://10.0.0.2/v1\" is reached over HTTPS, or over plain HTTP on this machine; write insecure = true to reach it over plain HTTP elsewhere",
        "models.ftp.base: \"ftp://10.0.0.2/v1\" is reached over HTTPS",
        "tasks.none.models: a task asks one model or more, in turn, such as models = [\"jev\", \"local\"]",
        "tasks.lost.models: no model is named \"missing\"; name it under [models.missing], or one of \"fine\"",
        "tasks.lost.options: a question offers from 2 to 32 options",
        "tasks.lost.least: an answer's least probability is from 0 to 1, such as least = 0.8",
        "views.mine.decide.turn: the task \"state\" decides whose turn it is, so each of its options is a state but unknown, such as waiting-on-review",
        "views.mine.decide.noise: no task is named \"nothing\"; the tasks are \"state\"",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
    let unmodelled = errors(&with(
        "[tasks.lost]\nmodels = [\"missing\"]\ninstructions = \"?\"\noptions = { a = \"x\", b = \"y\" }\n",
    ));
    assert!(
        unmodelled.contains(
            "tasks.lost.models: no model is named \"missing\"; name it under [models.missing]\n"
        ) || unmodelled.ends_with("name it under [models.missing]"),
        "{unmodelled}"
    );
}

#[test]
fn anthropic_and_gemini_are_reached_at_their_own_base_with_their_own_key_header() {
    let config = loaded(&with(
        r#"[models.claude]
kind = "anthropic"
model = "claude-x"
private = true

[models.gemini]
kind = "gemini"
model = "gemini-x"
tokens = 256

[models.foundry]
kind = "anthropic"
model = "claude-x"
base = "https://acme.services.ai.azure.com/anthropic/v1"
header = "api-key"
"#,
    ));
    let read: Vec<Native<'_>> = config
        .models
        .iter()
        .map(|(name, model)| {
            (
                name.as_str(),
                model.speaks,
                model.base.as_str(),
                model.header.as_deref(),
                model.private,
                model.tokens,
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (
                "claude",
                Speaks::Anthropic,
                "https://api.anthropic.com/v1",
                Some("x-api-key"),
                true,
                MOST_TOKENS
            ),
            (
                "foundry",
                Speaks::Anthropic,
                "https://acme.services.ai.azure.com/anthropic/v1",
                Some("api-key"),
                false,
                MOST_TOKENS
            ),
            (
                "gemini",
                Speaks::Gemini,
                "https://generativelanguage.googleapis.com/v1beta",
                Some("x-goog-api-key"),
                false,
                256
            ),
        ]
    );
}

#[test]
fn bedrock_is_reached_in_its_region_signed_unless_it_names_a_key() {
    let config = loaded(&with(
        r#"[models.signed]
kind = "bedrock"
region = "us-gov-west-1"
model = "anthropic.claude-x-v1:0"
session_token = { secret = "HERALD_AWS_TOKEN" }

[models.keyed]
kind = "bedrock"
region = "eu-west-1"
model = "m"
base = "https://bedrock-runtime-fips.eu-west-1.amazonaws.com"
key = { secret = "HERALD_BEDROCK_KEY" }
access_key_id = { secret = "HERALD_AWS_ID" }
"#,
    ));
    let signed = &config.models["signed"];
    assert_eq!(signed.speaks, Speaks::Bedrock);
    assert_eq!(
        signed.base,
        "https://bedrock-runtime.us-gov-west-1.amazonaws.com"
    );
    assert_eq!(signed.header, None);
    let bedrock = signed.bedrock.as_ref().unwrap();
    assert_eq!(
        (
            bedrock.region.as_str(),
            bedrock.access_key_id.secret.as_str(),
            bedrock.secret_access_key.secret.as_str(),
            bedrock.session_token.secret.as_str()
        ),
        (
            "us-gov-west-1",
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "HERALD_AWS_TOKEN"
        )
    );
    let keyed = &config.models["keyed"];
    assert_eq!(
        keyed.base,
        "https://bedrock-runtime-fips.eu-west-1.amazonaws.com"
    );
    assert_eq!(
        keyed.bedrock.as_ref().unwrap().access_key_id.secret,
        "HERALD_AWS_ID"
    );
    let polled = config.secrets_used(Uses::Polls);
    for read in [
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "HERALD_BEDROCK_KEY",
    ] {
        assert!(polled.contains(read), "{read}");
    }
    for unread in ["HERALD_AWS_ID", "HERALD_AWS_TOKEN"] {
        assert!(!polled.contains(unread), "{unread}");
    }
    assert!(config.secrets().contains("HERALD_AWS_TOKEN"));
}

#[test]
fn a_model_its_host_runs_names_its_binding_and_neither_a_base_nor_a_key() {
    let config = loaded(&with(
        r#"[models.ai]
kind = "workers-ai"
binding = "AI"
model = "@cf/meta/llama-x"

[models.rest]
kind = "workers-ai"
base = "https://api.cloudflare.com/client/v4/accounts/a/ai/v1"
model = "@cf/meta/llama-x"

[models.spin]
kind = "spin"
model = "llama2-chat"
"#,
    ));
    let read: Vec<Bound<'_>> = config
        .models
        .iter()
        .map(|(name, model)| {
            (
                name.as_str(),
                model.speaks,
                model.base.as_str(),
                model.binding.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            ("ai", Speaks::WorkersAi, "", Some("AI")),
            (
                "rest",
                Speaks::OpenAi,
                "https://api.cloudflare.com/client/v4/accounts/a/ai/v1",
                None
            ),
            ("spin", Speaks::Spin, "", None),
        ]
    );
    assert!(
        config
            .models
            .values()
            .all(|model| model.secret_refs().is_empty())
    );
}

#[test]
fn a_key_a_kind_does_not_take_is_named() {
    let error = errors(&with(
        r#"[models.unregioned]
kind = "bedrock"
model = "m"

[models.odd]
kind = "bedrock"
region = "EU/west"
model = "m"

[models.local]
kind = "ollama"
model = "m"
region = "eu-west-1"
session_token = { secret = "T" }
binding = "AI"

[models.spin]
kind = "spin"
model = "m"
base = "https://models.example"
key = { secret = "K" }
"#,
    ));
    for wanted in [
        "models.unregioned.region: a \"bedrock\" model names the region it is reached in, such as region = \"eu-west-1\"",
        "models.odd.region: \"EU/west\" is no region; write it as AWS names it, such as region = \"eu-west-1\"",
        "models.local.region: only a \"bedrock\" model takes region",
        "models.local.session_token: only a \"bedrock\" model takes session_token",
        "models.local.binding: only a \"workers-ai\" model takes binding",
        "models.spin.base: a model its host runs through a binding takes no base",
        "models.spin.key: a model its host runs through a binding takes no key",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
}
