//! What doctor says of each task and model: what each model is shown,
//! which may see private repositories, and what cannot be asked or kept
//! (ADR-0057).

use herald_flows::{Here, Tier};

use super::{loaded, on, server, worker};

fn tasks(store: &str) -> herald_config::Config {
    loaded(&format!(
        r#"version = 1
[store]
{store}
[sources.graphed]
kind = "github"
repository = "o/graphed"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[models.jev]
kind = "typesafe"
model = "jev-latest"
key = {{ secret = "HERALD_TYPESAFE_KEY" }}
[models.lan]
kind = "openai-compatible"
model = "m-1"
base = "http://10.0.0.2/v1"
insecure = true
private = true
[tasks.urgency]
models = ["jev", "lan"]
instructions = "How urgent is it?"
options = {{ high = "it blocks a release", low = "it can wait" }}
most = 5
[views.queue]
decide = {{ priority = "urgency" }}
"#
    ))
}

fn task_notes(config: &herald_config::Config, here: &Here) -> Vec<String> {
    on(config, &server(), here)
        .notes
        .into_iter()
        .filter(|note| {
            ["This build", "The store keeps no", "The view", "The model"]
                .iter()
                .any(|start| note.starts_with(start))
        })
        .collect()
}

#[test]
fn each_task_names_its_models_and_what_they_see_and_each_model_what_it_may_see() {
    let built = Here {
        models: true,
        ..Here::default()
    };
    assert_eq!(
        task_notes(&tasks("kind = \"sqlite\""), &built),
        [
            "The view queue asks the task urgency for priority of at most 5 pull requests a round, of jev, lan in turn, each shown each pull request's repository, number, title, author, labels, reviewers, whether it is a draft or a bot's, and the state and reason herald's rules gave it.",
            "The model jev (typesafe, https://api.typesafe.ai/v1) is shown public repositories' pull requests alone.",
            "The model jev names jev-latest, which moves; pin a version so a decision is made by the model it names.",
            "The model lan (openai-compatible, http://10.0.0.2/v1) may be shown private and internal repositories' pull requests.",
            "The model lan is reached over plain HTTP at http://10.0.0.2/v1, as insecure = true lets it.",
        ]
    );
    let notes = task_notes(&tasks("kind = \"markers\""), &Here::default());
    assert_eq!(
        notes[..2],
        [
            "This build asks no model, so every task answers unknown; build herald with its models feature to ask them.",
            "The store keeps no decisions, so no task is asked; a store that keeps records keeps them.",
        ]
    );
    let untasked = loaded(
        "version = 1\n[store]\nkind = \"sqlite\"\n[models.jev]\nkind = \"typesafe\"\nmodel = \"jev-1.14.0\"\n",
    );
    assert_eq!(
        task_notes(&untasked, &Here::default()),
        Vec::<String>::new()
    );
}

fn bound() -> herald_config::Config {
    loaded(
        r#"version = 1
[store]
kind = "sqlite"
[sources.graphed]
kind = "github"
repository = "o/graphed"
[models.ai]
kind = "workers-ai"
binding = "AI"
model = "@cf/meta/llama-x"
[models.spin]
kind = "spin"
model = "llama2-chat"
[tasks.urgency]
models = ["ai", "spin"]
instructions = "How urgent is it?"
options = { high = "it blocks a release", low = "it can wait" }
[views.queue]
decide = { priority = "urgency" }
"#,
    )
}

#[test]
fn a_model_its_host_runs_is_named_by_its_binding_and_asked_only_where_it_runs() {
    let built = Here {
        models: true,
        ..Here::default()
    };
    assert_eq!(
        task_notes(&bound(), &built)[1..],
        [
            "The model ai (workers-ai, through the Workers AI binding AI) is shown public repositories' pull requests alone.",
            "The model spin (spin, through Spin's llm interface) is shown public repositories' pull requests alone.",
        ]
    );
    let gaps = |layer| {
        on(&bound(), &layer, &built)
            .gaps
            .into_iter()
            .filter(|gap| gap.key.starts_with("models."))
            .map(|gap| (gap.feature, gap.key, gap.why, gap.first_layer))
            .collect::<Vec<_>>()
    };
    let spin = (
        "asking the model spin".to_owned(),
        "models.spin.kind".to_owned(),
        None,
    );
    assert_eq!(
        gaps(server()),
        [
            (
                "asking the model ai".to_owned(),
                "models.ai.kind".to_owned(),
                "the server layer runs no workers-ai model beside herald, through a binding"
                    .to_owned(),
                Some(Tier::Serverless)
            ),
            (
                spin.0.clone(),
                spin.1.clone(),
                "the server layer runs no spin model beside herald, through a binding".to_owned(),
                spin.2
            ),
        ]
    );
    assert_eq!(
        gaps(worker()),
        [(
            spin.0,
            spin.1,
            "the worker layer runs no spin model beside herald, through a binding".to_owned(),
            spin.2
        )]
    );
}
