//! What a view's tasks decide of the rows its rules leave untold: each
//! decision kept in the store and reused while its pull request is
//! unchanged, asked of the task's models in turn where none is kept, and
//! unknown where none answers among its options sure enough, where no
//! model may see a private repository's data, or where the store keeps no
//! decisions (ADR-0057); and each pull request's last leaving of herald's
//! own queue the store keeps, on its row (ADR-0087). Where a decision
//! cannot be had, the board's "could not be read" list says why, once per
//! repository and reason.

use std::collections::BTreeMap;

use herald_aws::{Endpoint, Signing, signed};
use herald_config::{Config, Task, ViewConfig};
use herald_core::{
    Answer, Decided, Key, Pull, Question, Row, Unread, Visibility, decided, pull_identity, rows,
    state,
};
use herald_models::{Asked, Model, bedrock};
use herald_ports::{Clock, Decisions, Http, Request, Signer, Store};
use jiff::Timestamp;

use crate::announce::Ports;
use crate::merging::with_left;
use crate::prepare::{Asking, Connected, PreparedModel};

/// The rows a view shows, each slot its tasks decide settled where an
/// answer may be taken, and why each decision that could not be had was
/// not, once per repository and reason.
pub(crate) struct Settled {
    /// The rows.
    pub(crate) rows: Vec<Row>,
    /// Why a task decided nothing of a repository's rows.
    pub(crate) unread: Vec<Unread>,
}

/// Why a task's decision of a row could not be had; never a credential or
/// a model's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unhad {
    /// The store keeps no decisions.
    Unkept,
    /// The decision the store keeps could not be read.
    Unreadable,
    /// Each model asked failed or gave no answer.
    Unanswered,
    /// The answer could not be kept, so it is not taken (ADR-0057).
    Unstored,
}

impl Unhad {
    /// Why, as the board says it.
    const fn why(self) -> &'static str {
        match self {
            Self::Unkept => "the store keeps no decisions",
            Self::Unreadable => "the decision the store keeps could not be read",
            Self::Unanswered => "no model it asked answered",
            Self::Unstored => "the answer could not be kept, so it is not taken",
        }
    }
}

/// The rows `view` shows of `pulls` now, each with its last leaving of
/// herald's own queue where the store keeps one, each slot its tasks
/// decide settled where an answer may be taken, and why a decision could
/// not be had.
pub(crate) async fn decided_rows<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    view: &ViewConfig,
    pulls: &[Pull],
) -> Settled {
    let pulls = with_left((config, ports.store.actions()), view, pulls).await;
    let mut shown = rows(&view.view, &pulls, ports.clock.now());
    let mut unhad: BTreeMap<(String, String), Visibility> = BTreeMap::new();
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
            match answered((connected, ports), (name, task, &question), row).await {
                Ok(Some(answer)) => {
                    if let Some(choice) = answer.settled(&question, task.least) {
                        decided(row, slot, (name, choice));
                    }
                }
                Ok(None) => {}
                Err(why) => {
                    let reason = format!("task \"{name}\" decided nothing: {}", why.why());
                    let seen = row.pull.visibility;
                    unhad
                        .entry((row.pull.repository.clone(), reason))
                        .and_modify(|visibility| *visibility = visibility.stricter(seen))
                        .or_insert(seen);
                }
            }
        }
    }
    Settled {
        rows: shown,
        unread: unhad
            .into_iter()
            .map(|((repository, reason), visibility)| Unread {
                repository,
                visibility,
                reason,
            })
            .collect(),
    }
}

/// What a model answered task `name` of `row`: the decision the store
/// keeps for it, or else the first of the task's models allowed to see it
/// that answers, kept before it is taken; none where no model may be
/// asked.
///
/// # Errors
///
/// Why none was had: the store keeps no decisions, or cannot read or
/// write them, or no model asked answers.
async fn answered<S: Store, C: Clock, G: Signer, H: Http>(
    (connected, ports): (&Connected<H>, &Ports<'_, S, C, G>),
    (name, task, question): (&str, &Task, &Question<'_>),
    row: &Row,
) -> Result<Option<Answer>, Unhad> {
    let store = ports.store.decisions().ok_or(Unhad::Unkept)?;
    let subject = pull_identity(&row.pull);
    let input = state(row);
    let key = Key::decided(name, &subject, &input);
    if let Some(kept) = store.decision(&key).await.map_err(|_| Unhad::Unreadable)? {
        return Ok(Some(kept.answer));
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
    let mut unanswered = Ok(None);
    for (model, asking) in models {
        unanswered = Err(Unhad::Unanswered);
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
        store
            .put_decision(&decision)
            .await
            .map_err(|_| Unhad::Unstored)?;
        return Ok(Some(decision.answer));
    }
    unanswered
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
