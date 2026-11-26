//! herald's own queue's steps: a pull request's branch brought up to date
//! with its base, and the pull request merged, each through the forge's
//! API with the queue's write credential, naming the head the forge must
//! find wherever the forge takes one (ADR-0087).

use herald_core::Rerun;
use herald_core::merging::{Message, Method, StepKind, Update};
use herald_kinds::{Done, Entered, Merge, Unpolled};
use herald_ports::{HttpError, Method as Verb, Request, Response, Secret};
use serde_json::{Map, Value, json};

use super::{ForgeMergeWriter, refusing, said_of};
use crate::Forge;

/// What GitHub's refusal of an update says where the branch conflicts.
const CONFLICTING: &str = "conflict";

/// Why a merge by a method the forge has no call for is not sent.
const NO_SUCH_METHOD: &str = "the forge has no merge by this method, so nothing was sent";

/// `method` as GitHub's `merge_method` and Forgejo's `Do` name it.
fn named(forge: Forge, method: Method) -> Option<&'static str> {
    match (forge, method) {
        (Forge::GitHub | Forge::Forgejo, Method::Merge) => Some("merge"),
        (Forge::GitHub | Forge::Forgejo, Method::Squash) => Some("squash"),
        (Forge::GitHub | Forge::Forgejo, Method::Rebase) => Some("rebase"),
        (Forge::Forgejo, Method::FastForward) => Some("fast-forward-only"),
        _ => None,
    }
}

/// The title and body `merge` gives the commit, where herald gives them.
fn message<'m>(merge: &Merge<'m>) -> Option<(&'m str, &'m str)> {
    (merge.message == Message::Pull && merge.method.messaged())
        .then_some((&merge.entered.title, &merge.entered.body))
}

/// Whether `answer` to a step of `kind` on `forge` says the branch
/// conflicts with the base.
fn conflicts(forge: Forge, kind: StepKind, answer: &Response) -> bool {
    kind == StepKind::Update
        && match forge {
            Forge::Forgejo => answer.status == 409,
            Forge::GitHub => {
                answer.status == 422
                    && String::from_utf8_lossy(&answer.body)
                        .to_lowercase()
                        .contains(CONFLICTING)
            }
            Forge::GitLab => false,
        }
}

impl ForgeMergeWriter {
    /// The request bringing `entered`'s branch up to date as `update` says.
    pub(super) fn updating(&self, entered: &Entered, update: Update) -> Result<Request, Unpolled> {
        let checked = self.origin.checked()?;
        let number = entered.number;
        Ok(match self.origin.forge() {
            Forge::GitHub => checked.sent(
                Verb::Put,
                &format!("/pulls/{number}/update-branch"),
                &json!({ "expected_head_sha": entered.head }),
                &self.token,
            ),
            Forge::GitLab => checked.sent(
                Verb::Put,
                &format!("/merge_requests/{number}/rebase"),
                &json!({}),
                &self.token,
            ),
            Forge::Forgejo => {
                let style = match update {
                    Update::Merge => "merge",
                    Update::Rebase => "rebase",
                };
                checked.sent(
                    Verb::Post,
                    &format!("/pulls/{number}/update?style={style}"),
                    &json!({}),
                    &self.token,
                )
            }
        })
    }

    /// The request re-running the failed job of `job` alone (ADR-0068).
    pub(super) fn rerunning(&self, job: &Rerun) -> Result<Request, Unpolled> {
        let path = crate::rerunner::rerun_path(self.origin.forge(), job)?;
        Ok(self
            .origin
            .checked()?
            .sent(Verb::Post, &path, &json!({}), &self.token))
    }

    /// The request merging the pull request `merge` names, its head named
    /// as the head the forge must find.
    pub(super) fn merging(&self, merge: Merge<'_>) -> Result<Request, Unpolled> {
        let checked = self.origin.checked()?;
        let forge = self.origin.forge();
        let entered = merge.entered;
        let number = entered.number;
        let said = message(&merge);
        let mut body = Map::new();
        let path = match forge {
            Forge::GitHub | Forge::Forgejo => {
                let method = named(forge, merge.method)
                    .ok_or_else(|| Unpolled::Unreadable(NO_SUCH_METHOD.to_owned()))?;
                if forge == Forge::GitHub {
                    body.insert("sha".to_owned(), json!(entered.head));
                    body.insert("merge_method".to_owned(), json!(method));
                    if let Some((title, text)) = said {
                        body.insert("commit_title".to_owned(), json!(title));
                        body.insert("commit_message".to_owned(), json!(text));
                    }
                } else {
                    body.insert("head_commit_id".to_owned(), json!(entered.head));
                    body.insert("Do".to_owned(), json!(method));
                    if let Some((title, text)) = said {
                        body.insert("MergeTitleField".to_owned(), json!(title));
                        body.insert("MergeMessageField".to_owned(), json!(text));
                    }
                }
                format!("/pulls/{number}/merge")
            }
            Forge::GitLab => {
                let squash = merge.method == Method::Squash;
                body.insert("sha".to_owned(), json!(entered.head));
                body.insert("squash".to_owned(), json!(squash));
                if let Some((title, text)) = said {
                    let key = if squash {
                        "squash_commit_message"
                    } else {
                        "merge_commit_message"
                    };
                    body.insert(key.to_owned(), json!(format!("{title}\n\n{text}")));
                }
                format!("/merge_requests/{number}/merge")
            }
        };
        let verb = if forge == Forge::Forgejo {
            Verb::Post
        } else {
            Verb::Put
        };
        Ok(checked.sent(verb, &path, &Value::Object(body), &self.token))
    }

    /// What `answer` to a step of `kind` says, the write credential hidden
    /// in any refusal.
    pub(super) fn done(&self, answer: &Result<Response, HttpError>, kind: StepKind) -> Done {
        let Ok(answer) = answer else {
            return Done::Lost;
        };
        let forge = self.origin.forge();
        let held = self.held();
        let held = held.as_ref().map(Secret::expose);
        if conflicts(forge, kind, answer) {
            Done::Conflict(said_of(answer, held))
        } else if refusing(answer.status) {
            Done::Refused(said_of(answer, held))
        } else if answer.succeeded() {
            Done::Took
        } else {
            Done::Lost
        }
    }
}

#[cfg(test)]
mod tests;
