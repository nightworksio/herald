//! What a view's tasks decide of the rows its rules leave untold: each
//! decision kept in the store and reused while its pull request is
//! unchanged, asked of the task's models in turn where none is kept, and
//! unknown where none answers among its options sure enough, where no
//! model may see a private repository's data, or where the store keeps no
//! decisions (ADR-0057).

use herald_config::{Config, Task, ViewConfig};
use herald_core::{
    Answer, Decided, Key, Pull, Question, Row, Visibility, decided, pull_identity, rows, state,
};
use herald_ports::{Clock, Decisions, Http, Signer, Store};

use crate::announce::Ports;
use crate::prepare::Connected;

/// The rows `view` shows of `pulls` now, each slot its tasks decide
/// settled where an answer may be taken.
pub(crate) async fn decided_rows<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    view: &ViewConfig,
    pulls: &[Pull],
) -> Vec<Row> {
    let mut shown = rows(&view.view, pulls, ports.clock.now());
    let tasks = view
        .decide
        .iter()
        .filter_map(|(slot, name)| Some((*slot, name.as_str(), config.tasks.get(name)?)));
    for (slot, name, task) in tasks {
        let question = Question {
            id: name,
            instructions: &task.instructions,
            options: &task.options,
        };
        let asked = shown
            .iter_mut()
            .filter(|row| slot.asks(row))
            .take(task.most);
        for row in asked {
            let Some(answer) = answered((connected, ports), (name, task, &question), row).await
            else {
                continue;
            };
            if let Some(choice) = answer.settled(&question, task.least) {
                decided(row, slot, (name, choice));
            }
        }
    }
    shown
}

/// What a model answered task `name` of `row`: the decision the store
/// keeps for it, or else the first of the task's models allowed to see it
/// that answers, kept before it is taken; none where the store keeps no
/// decisions, cannot be read or written, or no model answers.
async fn answered<S: Store, C: Clock, G: Signer, H: Http>(
    (connected, ports): (&Connected<H>, &Ports<'_, S, C, G>),
    (name, task, question): (&str, &Task, &Question<'_>),
    row: &Row,
) -> Option<Answer> {
    let store = ports.store.decisions()?;
    let subject = pull_identity(&row.pull);
    let input = state(row);
    let key = Key::decided(name, &subject, &input);
    if let Some(kept) = store.decision(&key).await.ok()? {
        return Some(kept.answer);
    }
    let public = row.pull.visibility == Visibility::Public;
    let models = task
        .models
        .iter()
        .filter_map(|named| {
            connected
                .models
                .iter()
                .find(|(model, _)| model.name == *named)
                .filter(|(model, _)| public || model.private)
        })
        .take_while(|_| ports.budget.allows(ports.clock.now(), 1).is_ok());
    for (model, http) in models {
        let questions = [*question];
        let Ok(response) = http.send(model.model.request(&input, &questions)).await else {
            continue;
        };
        let Some(answer) = model
            .model
            .answers(&response, &questions)
            .ok()
            .and_then(|mut answers| answers.remove(name))
        else {
            continue;
        };
        let decision = Decided {
            key,
            task: name.to_owned(),
            subject,
            answer,
            at: ports.clock.now(),
        };
        store.put_decision(&decision).await.ok()?;
        return Some(decision.answer);
    }
    None
}
