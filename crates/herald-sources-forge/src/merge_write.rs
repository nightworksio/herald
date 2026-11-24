//! A merge queue's write calls: a pull request given to its forge's merge
//! queue or merge train with the queue's write credential, naming the head
//! the forge must find, or taken out of it again, sent to the forge's API
//! alone (ADR-0087).

use herald_kinds::{Answered, Entered, MergeWriter, TakenBack, Unpolled};
use herald_ports::{Boxed, DynHttp, Erased, Http as _, HttpError, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Forge, Origin, Token};

/// The mutation handing a pull request to GitHub's merge queue, its head
/// named as the head GitHub must find.
const ENQUEUE: &str = "mutation($id: ID!, $head: GitObjectID!) { enqueuePullRequest(input: { pullRequestId: $id, expectedHeadOid: $head }) { mergeQueueEntry { id } } }";

/// The mutation taking a pull request out of GitHub's merge queue.
const DEQUEUE: &str =
    "mutation($id: ID!) { dequeuePullRequest(input: { id: $id }) { mergeQueueEntry { id } } }";

/// What GitLab's cancel of an auto-merge says where it did it.
const CANCELLED: &str = "success";

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
struct Dequeued {
    data: Option<DequeuedData>,
    #[serde(default)]
    errors: Vec<Said>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DequeuedData {
    dequeue_pull_request: Option<EnqueuedEntry>,
}

/// GitLab's answer to cancelling an auto-merge: whether it did, and why
/// not.
#[derive(Deserialize)]
struct Cancelled {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    message: Option<Value>,
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
fn refusal(answer: &Response, held: Option<&str>) -> Answered {
    let said = serde_json::from_slice::<Said>(&answer.body)
        .map(|said| messages(&[said]))
        .unwrap_or_default();
    let said = if said.is_empty() {
        format!("the forge answered {}", answer.status)
    } else {
        format!("the forge answered {}: {said}", answer.status)
    };
    Answered::Refused(shown(&said, held))
}

/// A reader of a GitHub mutation's answer body: its errors, and whether it
/// names the merge queue entry it acted on.
type MutationRead = fn(&[u8]) -> Option<(Vec<Said>, bool)>;

/// A reader of GitLab's answer to a write, the write credential held as
/// the second argument hidden in any refusal.
type WriteRead = fn(&Response, Option<&str>) -> Answered;

/// What GitHub's answer to a mutation says, its body read by `read` as
/// its errors and whether it names the merge queue entry it acted on.
fn github_answered(answer: &Response, held: Option<&str>, read: MutationRead) -> Answered {
    if refusing(answer.status) {
        return refusal(answer, held);
    }
    if !answer.succeeded() {
        return Answered::Lost;
    }
    let Some((errors, entry)) = read(&answer.body) else {
        return Answered::Lost;
    };
    if !errors.is_empty() {
        let said = format!("GitHub refused it: {}", messages(&errors));
        return Answered::Refused(shown(&said, held));
    }
    if entry {
        Answered::Done
    } else {
        Answered::Lost
    }
}

/// The errors of GitHub's answer to the enqueue, and whether it names the
/// entry it made.
fn enqueued(body: &[u8]) -> Option<(Vec<Said>, bool)> {
    let enqueued = serde_json::from_slice::<Enqueued>(body).ok()?;
    let entry = enqueued
        .data
        .and_then(|data| data.enqueue_pull_request)
        .and_then(|enqueued| enqueued.merge_queue_entry);
    Some((enqueued.errors, entry.is_some()))
}

/// The errors of GitHub's answer to the dequeue, and whether it names the
/// entry it took out.
fn dequeued(body: &[u8]) -> Option<(Vec<Said>, bool)> {
    let dequeued = serde_json::from_slice::<Dequeued>(body).ok()?;
    let entry = dequeued
        .data
        .and_then(|data| data.dequeue_pull_request)
        .and_then(|dequeued| dequeued.merge_queue_entry);
    Some((dequeued.errors, entry.is_some()))
}

/// What GitLab's answer to cancelling the auto-merge says: a 201 whose
/// status is not `success` is a refusal, saying its message.
fn gitlab_cancelled(answer: &Response, held: Option<&str>) -> Answered {
    if refusing(answer.status) {
        return refusal(answer, held);
    }
    if !answer.succeeded() {
        return Answered::Lost;
    }
    match serde_json::from_slice::<Cancelled>(&answer.body) {
        Ok(cancelled) if cancelled.status.as_deref() == Some(CANCELLED) => Answered::Done,
        Ok(Cancelled {
            status: Some(status),
            message,
        }) => {
            let said = message.map_or_else(
                || format!("GitLab answered {status}"),
                |message| {
                    format!(
                        "GitLab answered {status}: {}",
                        messages(&[Said {
                            message: Some(message),
                            error: None,
                        }])
                    )
                },
            );
            Answered::Refused(shown(&said, held))
        }
        _ => Answered::Lost,
    }
}

/// What GitLab's answer to adding the merge request to the train says.
fn gitlab_handed(answer: &Response, held: Option<&str>) -> Answered {
    if refusing(answer.status) {
        refusal(answer, held)
    } else if answer.succeeded() {
        Answered::Done
    } else {
        Answered::Lost
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

    /// The GraphQL request posting `body` to GitHub with the write
    /// credential.
    fn mutation(&self, body: &Value) -> herald_ports::Request {
        let request = crate::graphql::posted(&self.graphql, body, self.origin.reading().parse);
        crate::forge::authorized(Forge::GitHub, request, Some(&self.token))
    }

    /// The node id GitHub names the pull request by, or why there is none.
    fn node(id: Option<&str>, doing: &str) -> Result<String, Unpolled> {
        id.map(str::to_owned).ok_or_else(|| {
            Unpolled::Unreadable(format!(
                "GitHub named no node id for the pull request, so it cannot be {doing}"
            ))
        })
    }

    /// The request handing `entered` over, or why none can be made.
    fn request(&self, entered: &Entered) -> Result<herald_ports::Request, Unpolled> {
        match self.origin.forge() {
            Forge::GitHub => {
                let id = Self::node(entered.id.as_deref(), "handed to the merge queue")?;
                Ok(self.mutation(&json!({
                    "query": ENQUEUE,
                    "variables": { "id": id, "head": entered.head },
                })))
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

    /// The request taking `taken` out of the forge's queue, or why none
    /// can be made.
    fn taking(&self, taken: &TakenBack) -> Result<herald_ports::Request, Unpolled> {
        match self.origin.forge() {
            Forge::GitHub => {
                let id = Self::node(taken.id.as_deref(), "taken out of the merge queue")?;
                Ok(self.mutation(&json!({ "query": DEQUEUE, "variables": { "id": id } })))
            }
            Forge::GitLab => {
                let path = format!(
                    "/merge_requests/{}/cancel_merge_when_pipeline_succeeds",
                    taken.number
                );
                Ok(self
                    .origin
                    .checked()?
                    .posted(&path, &json!({}), &self.token))
            }
            Forge::Forgejo => Err(Unpolled::Unreadable(
                crate::options::FORGEJO_QUEUELESS.to_owned(),
            )),
        }
    }

    /// What `answer` says, read per forge as `github` or `gitlab` reads it,
    /// the write credential hidden in any refusal.
    fn answered(
        &self,
        answer: &Result<Response, HttpError>,
        (github, gitlab): (MutationRead, WriteRead),
    ) -> Answered {
        let Ok(answer) = answer else {
            return Answered::Lost;
        };
        let held = self.exposed();
        match self.origin.forge() {
            Forge::GitHub => github_answered(answer, held.as_deref(), github),
            Forge::GitLab | Forge::Forgejo => gitlab(answer, held.as_deref()),
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
    ) -> Boxed<'a, Result<Answered, Unpolled>> {
        Box::pin(async move {
            let request = self.request(entered)?;
            let answer = Erased(write).send(request).await;
            Ok(self.answered(&answer, (enqueued, gitlab_handed)))
        })
    }

    fn take_out<'a>(
        &'a self,
        write: &'a dyn DynHttp,
        taken: &'a TakenBack,
    ) -> Boxed<'a, Result<Answered, Unpolled>> {
        Box::pin(async move {
            let request = self.taking(taken)?;
            let answer = Erased(write).send(request).await;
            Ok(self.answered(&answer, (dequeued, gitlab_cancelled)))
        })
    }
}

#[cfg(test)]
mod tests;
