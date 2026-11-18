//! `[models]`, `[tasks]` and a view's `decide` (ADR-0057): each model read
//! against its kind and base, each task against its models, and every
//! mistake named by its key.

mod common;

#[cfg(test)]
mod models {
    use herald_config::{Config, MOST_ASKED, load};
    use herald_core::{SecretRef, Slot};
    use herald_models::{MOST_ANSWER, Speaks};

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
            "models.odd.kind: there is no model kind \"oracle\"; the kinds are \"typesafe\", \"openai-compatible\", \"ollama\"",
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
}
