//! What a view's tasks decide of the rows its rules leave untold: each
//! decision kept in the store and reused while its pull request is
//! unchanged, asked of the task's models in turn where none is kept, and
//! unknown where none answers among its options sure enough, where no
//! model may see a private repository's data, or where the store keeps no
//! decisions (ADR-0057); and each pull request's last leaving of herald's
//! own queue the store keeps, on its row (ADR-0087).

use std::collections::BTreeMap;

use herald_aws::{Endpoint, Signing, signed};
use herald_config::{Config, Task, ViewConfig};
use herald_core::{
    Answer, Decided, Key, Pull, Question, Row, Visibility, decided, pull_identity, rows, state,
};
use herald_models::{Asked, Model, bedrock};
use herald_ports::{Clock, Decisions, Http, Request, Signer, Store};
use jiff::Timestamp;

use crate::announce::Ports;
use crate::merging::with_left;
use crate::prepare::{Asking, Connected, PreparedModel};

/// The rows `view` shows of `pulls` now, each with its last leaving of
/// herald's own queue where the store keeps one, each slot its tasks
/// decide settled where an answer may be taken.
pub(crate) async fn decided_rows<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    view: &ViewConfig,
    pulls: &[Pull],
) -> Vec<Row> {
    let pulls = with_left((config, ports.store.actions()), view, pulls).await;
    let mut shown = rows(&view.view, &pulls, ports.clock.now());
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
    for (model, asking) in models {
        let questions = [*question];
        let Some(answer) = asked((model, asking), &input, &questions, ports.clock.now())
            .await
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

/// What `model`, reached as `asking` says, answers `questions` of `input`
/// at `now`; nothing where it is not reached or gives no answer.
async fn asked<H: Http>(
    (model, asking): (&PreparedModel, &Asking<H>),
    input: &str,
    questions: &[Question<'_>],
    now: Timestamp,
) -> Option<BTreeMap<String, Answer>> {
    match (model.model.asked(input, questions), asking) {
        (Asked::Sent(request), Asking::Http(http)) => {
            let response = http.send(sign(&model.model, request, now)).await.ok()?;
            model.model.answers(&response, questions).ok()
        }
        (Asked::Bound(inference), Asking::Bound(infer)) => {
            let answered = infer.infer_boxed(inference).await.ok()?;
            model.model.answered(&answered, questions).ok()
        }
        (Asked::Sent(_) | Asked::Bound(_), _) => None,
    }
}

/// `request` to `model`, signed at `now` with AWS Signature Version 4
/// where it is reached through Amazon Bedrock with no API key (ADR-0059).
fn sign(model: &Model, request: Request, now: Timestamp) -> Request {
    let Some(keys) = model.signed_by() else {
        return request;
    };
    let endpoint = Endpoint::new(bedrock::ENDPOINT, &keys.region, Some(&model.base));
    let signing = Signing {
        access_key_id: &keys.access_key_id,
        secret_access_key: &keys.secret_access_key,
        session_token: keys.session_token.as_ref(),
        region: &keys.region,
        service: bedrock::SERVICE,
        host: endpoint.host(),
    };
    let canonical = endpoint.canonical(&bedrock::canonical(&model.model));
    signed(
        request,
        &canonical,
        herald_core::media::JSON,
        &[],
        &signing,
        now,
    )
}
