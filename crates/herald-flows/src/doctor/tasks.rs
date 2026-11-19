//! What doctor says of each task: the models it asks and what each is
//! shown, which models may see private repositories' data, what cannot
//! be kept or asked, and which model ids float (ADR-0057).

use herald_config::Config;

use super::Here;

/// What every model is shown of a pull request.
const SHOWN: &str = "each pull request's repository, number, title, author, labels, reviewers, whether it is a draft or a bot's, and the state and reason herald's rules gave it";

/// What a model id ends with where it names no fixed version.
const FLOATING: [&str; 3] = ["latest", "preview", "beta"];

/// Each note on the tasks and models of `config`, as `here` builds.
pub(super) fn decided(config: &Config, here: &Here, notes: &mut Vec<String>) {
    let asked: Vec<_> = config
        .views
        .iter()
        .flat_map(|(view, viewed)| {
            viewed
                .decide
                .iter()
                .map(move |(slot, task)| (view.as_str(), slot.name(), task.as_str()))
        })
        .filter_map(|(view, slot, name)| Some((view, slot, name, config.tasks.get(name)?)))
        .collect();
    if asked.is_empty() {
        return;
    }
    if !here.models {
        notes.push("This build asks no model, so every task answers unknown; build herald with its models feature to ask them.".to_owned());
    }
    if !config.store.keeps_decisions() {
        notes.push("The store keeps no decisions, so no task is asked; a store that keeps records keeps them.".to_owned());
    }
    for (view, slot, name, task) in asked {
        notes.push(format!(
            "The view {view} asks the task {name} for {slot} of at most {} pull requests a round, of {} in turn, each shown {SHOWN}.",
            task.most,
            task.models.join(", ")
        ));
    }
    for (name, model) in &config.models {
        let reach = if model.private {
            "may be shown private and internal repositories' pull requests"
        } else {
            "is shown public repositories' pull requests alone"
        };
        notes.push(format!(
            "The model {name} ({}, {}) {reach}.",
            model.kind, model.base
        ));
        if model.insecure {
            notes.push(format!(
                "The model {name} is reached over plain HTTP at {}, as insecure = true lets it.",
                model.base
            ));
        }
        if FLOATING
            .iter()
            .any(|floating| model.model.ends_with(floating))
        {
            notes.push(format!(
                "The model {name} names {}, which moves; pin a version so a decision is made by the model it names.",
                model.model
            ));
        }
    }
}
