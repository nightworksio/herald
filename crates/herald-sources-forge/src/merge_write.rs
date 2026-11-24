//! A merge queue's hand-off: a pull request given to its forge's merge
//! queue or merge train with the queue's write credential, naming the head
//! the forge must find, sent to the forge's API alone (ADR-0087).

use herald_kinds::{Entered, Handed, MergeWriter, Unpolled};
use herald_ports::{Boxed, DynHttp, Erased, Http as _, HttpError, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Forge, Origin, Token};

/// The mutation handing a pull request to GitHub's merge queue, its head
/// named as the head GitHub must find.
const ENQUEUE: &str = "mutation($id: ID!, $head: GitObjectID!) { enqueuePullRequest(input: { pullRequestId: $id, expectedHeadOid: $head }) { mergeQueueEntry { id } } }";

/// The most of a forge's refusal herald shows.
const MOST_SAID: usize = 300;

/// What stands in for the write credential wherever a refusal holds it.
const HIDDEN: &str = "[the write credential]";

/// What hands pull requests of one repository to its forge's queue.
#[derive(Debug)]
pub(crate) struct ForgeMergeWriter {
    /// The repository, on its forge.
    pub(crate) origin: Origin,
    /// The forge's GraphQL endpoint, beside its API.
    pub(crate) graphql: String,
    /// The queue's write credential.
    pub(crate) token: Token,
}

#[derive(Deserialize)]
struct Enqueued {
    data: Option<EnqueuedData>,
    #[serde(default)]
    errors: Vec<Said>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnqueuedData {
    enqueue_pull_request: Option<EnqueuedEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnqueuedEntry {
    merge_queue_entry: Option<Value>,
}

#[derive(Deserialize)]
struct Said {
    #[serde(default)]
    message: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
}

/// Whether `status` says the forge refused the call outright, as opposed
/// to an answer that may yet change: a timeout or a rate limit.
fn refusing(status: u16) -> bool {
    (400..500).contains(&status) && !matches!(status, 408 | 429)
}

/// `text` as herald shows it: the write credential's value `held`, where
/// it is in it, hidden, control characters dropped, and at most
/// [`MOST_SAID`] characters.
fn shown(text: &str, held: Option<&str>) -> String {
    let hidden = match held.filter(|held| !held.is_empty()) {
        Some(held) => text.replace(held, HIDDEN),
        None => text.to_owned(),
    };
    hidden
        .chars()
        .filter(|character| !character.is_control())
        .take(MOST_SAID)
        .collect()
}

/// What `said` says, each message as its text.
fn messages(said: &[Said]) -> String {
    said.iter()
        .filter_map(|said| said.message.as_ref().or(said.error.as_ref()))
        .map(|message| match message {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The refusal `answer` holds, said without the credential's value `held`.
fn refusal(answer: &Response, held: Option<&str>) -> Handed {
    let said = serde_json::from_slice::<Said>(&answer.body)
        .map(|said| messages(&[said]))
        .unwrap_or_default();
    let said = if said.is_empty() {
        format!("the forge answered {}", answer.status)
    } else {
        format!("the forge answered {}: {said}", answer.status)
    };
    Handed::Refused(shown(&said, held))
}

/// What GitHub's answer to the enqueue says.
fn github_handed(answer: &Response, held: Option<&str>) -> Handed {
    if refusing(answer.status) {
        return refusal(answer, held);
    }
    if !answer.succeeded() {
        return Handed::Lost;
    }
    let Ok(enqueued) = serde_json::from_slice::<Enqueued>(&answer.body) else {
        return Handed::Lost;
    };
    if !enqueued.errors.is_empty() {
        let said = format!("GitHub refused it: {}", messages(&enqueued.errors));
        return Handed::Refused(shown(&said, held));
    }
    let entry = enqueued
        .data
        .and_then(|data| data.enqueue_pull_request)
        .and_then(|enqueued| enqueued.merge_queue_entry);
    if entry.is_some() {
        Handed::Queued
    } else {
        Handed::Lost
    }
}

/// What GitLab's answer to adding the merge request to the train says.
fn gitlab_handed(answer: &Response, held: Option<&str>) -> Handed {
    if refusing(answer.status) {
        refusal(answer, held)
    } else if answer.succeeded() {
        Handed::Queued
    } else {
        Handed::Lost
    }
}

impl ForgeMergeWriter {
    /// The write credential's value, where it holds one now.
    fn exposed(&self) -> Option<String> {
        match &self.token {
            Token::Named(secret) | Token::Run(secret) => Some(secret.expose().to_owned()),
            Token::Minted(minted) => minted.current().map(|secret| secret.expose().to_owned()),
        }
    }

    /// The request handing `entered` over, or why none can be made.
    fn request(&self, entered: &Entered) -> Result<herald_ports::Request, Unpolled> {
        match self.origin.forge() {
            Forge::GitHub => {
                let id = entered.id.as_deref().ok_or_else(|| {
                    Unpolled::Unreadable(
                        "GitHub named no node id for the pull request, so it cannot be handed to the merge queue".to_owned(),
                    )
                })?;
                let body = json!({
                    "query": ENQUEUE,
                    "variables": { "id": id, "head": entered.head },
                });
                let request =
                    crate::graphql::posted(&self.graphql, &body, self.origin.reading().parse);
                Ok(crate::forge::authorized(
                    Forge::GitHub,
                    request,
                    Some(&self.token),
                ))
            }
            Forge::GitLab => {
                let body = json!({ "sha": entered.head, "auto_merge": true });
                let path = format!("/merge_trains/merge_requests/{}", entered.number);
                Ok(self.origin.checked()?.posted(&path, &body, &self.token))
            }
            Forge::Forgejo => Err(Unpolled::Unreadable(
                crate::options::FORGEJO_QUEUELESS.to_owned(),
            )),
        }
    }

    /// What `answer` says, the write credential hidden in any refusal.
    fn handed(&self, answer: &Result<Response, HttpError>) -> Handed {
        let Ok(answer) = answer else {
            return Handed::Lost;
        };
        let held = self.exposed();
        match self.origin.forge() {
            Forge::GitHub => github_handed(answer, held.as_deref()),
            Forge::GitLab | Forge::Forgejo => gitlab_handed(answer, held.as_deref()),
        }
    }
}

impl MergeWriter for ForgeMergeWriter {
    fn api(&self) -> &str {
        self.origin.api()
    }

    fn hand_off<'a>(
        &'a self,
        write: &'a dyn DynHttp,
        entered: &'a Entered,
    ) -> Boxed<'a, Result<Handed, Unpolled>> {
        Box::pin(async move {
            let request = self.request(entered)?;
            let answer = Erased(write).send(request).await;
            Ok(self.handed(&answer))
        })
    }
}

#[cfg(test)]
mod tests;
