//! One planned job re-run, after the checks made right before the call:
//! the operator's role on its repository, the pull request's head, the job
//! and its run, and herald's own record, written before the call; a write
//! App's token is minted only after all of them (ADR-0068).

use herald_core::{Reran, Rerun, RerunRecord, RunKey};
use herald_kinds::{Granted, RunNow};
use herald_ports::{
    Actions, Clock, DynHttp, Http, HttpError, OperatorToken, Response, Signer, Store, Version,
    Written,
};

use super::Planned;
use super::signing::said;
use super::writing::{Signed, credential};
use crate::announce::Ports;
use crate::budget::Budgeted;

/// What became of one job an operator named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// The forge re-runs the job.
    Rerun,
    /// It was not asked for, for this reason.
    Refused(String),
    /// The forge refused the call with this status; nothing was re-run.
    Declined(u16),
    /// The answer was lost and the job did not read as re-run, so herald
    /// never asks for it again.
    Unsettled,
    /// The call came to this, but herald's record of it could not be
    /// updated, so it still says the call is being made and herald never
    /// asks for it again.
    Unrecorded(Reran),
}

/// One job an operator named, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The repository's full path, where the job was planned.
    pub repository: Option<String>,
    /// The pull request's number, where the job was planned.
    pub number: Option<u64>,
    /// The job's id.
    pub job: String,
    /// The head it was named on.
    pub head: String,
    /// What became of it.
    pub done: Done,
}

impl Applied {
    /// The job `job` on `head`, which no plan holds now, `withheld` pull
    /// requests of private or internal repositories withheld from the plans.
    pub(super) fn unplanned((job, head): (&str, &str), withheld: usize) -> Self {
        let why = if withheld == 0 {
            super::UNPLANNED
        } else {
            super::UNPLANNED_WITHHELD
        };
        Self {
            repository: None,
            number: None,
            job: job.to_owned(),
            head: head.to_owned(),
            done: Done::Refused(why.to_owned()),
        }
    }

    /// `planned`'s job `rerun`, which came to `done`.
    fn of(planned: &Planned, rerun: &Rerun, done: Done) -> Self {
        Self {
            repository: Some(planned.repository.clone()),
            number: Some(planned.number),
            job: rerun.job.clone(),
            head: planned.head.clone(),
            done,
        }
    }

    /// `planned`'s job `rerun`, not asked for, as `reason` says.
    pub(super) fn refused(planned: &Planned, rerun: &Rerun, reason: &str) -> Self {
        Self::of(planned, rerun, Done::Refused(reason.to_owned()))
    }
}

/// What the forge's answer to the call says: re-run, refused outright, or
/// lost, in which case the run is read back.
fn answered(answer: &Result<Response, HttpError>) -> Option<Reran> {
    match answer {
        Ok(response) if response.succeeded() => Some(Reran::Rerun),
        Ok(response)
            if (400..500).contains(&response.status) && !matches!(response.status, 408 | 429) =>
        {
            Some(Reran::Refused {
                status: response.status,
            })
        }
        _ => None,
    }
}

/// Why the job as read right before the call may not be re-run, where it
/// may not.
fn unrerunnable(now: &RunNow, head: &str) -> Option<String> {
    if now.head != head {
        Some(format!(
            "the job ran on {}, not on the head {head} the plan named",
            now.head
        ))
    } else if !now.failed {
        Some("the job no longer reads as failed, so nothing is re-run".to_owned())
    } else if now.reruns > 0 {
        Some(format!(
            "the job or its run was already re-run {} times, so herald leaves it",
            now.reruns
        ))
    } else {
        None
    }
}

/// The job `rerun` of `planned` as read right before its call through
/// `api`, where every check passes: the operator holding `token` holds the
/// role, the pull request is open on the head named, and the job is of the
/// run named, ran on that head, failed, and neither it nor its run was
/// re-run.
///
/// # Errors
///
/// The first check that fails, or a read that could not be made, saying
/// why in plain words.
async fn checked<H: Http>(
    (signed, token): (&Signed<'_, H>, &OperatorToken),
    api: &dyn DynHttp,
    (planned, rerun): (&Planned, &Rerun),
) -> Result<RunNow, String> {
    let repository = planned.repository.as_str();
    match signed.rerunner.granted(api, token, repository).await {
        Ok(Granted::Enough) => {}
        Ok(Granted::Short(held)) => {
            return Err(format!(
                "{} holds {held} on {repository}, and re-running needs {} or above",
                signed.operator,
                signed.least.name()
            ));
        }
        Err(unpolled) => {
            return Err(format!(
                "the operator's role on {repository} could not be read, so nothing was re-run: {}",
                said(unpolled)
            ));
        }
    }
    match signed
        .rerunner
        .head(api, (repository, planned.number))
        .await
    {
        Ok(Some(head)) if head == planned.head => {}
        Ok(Some(head)) => {
            return Err(format!(
                "the pull request's head moved to {head} since the plan; list the plan again"
            ));
        }
        Ok(None) => return Err("the pull request is no longer open".to_owned()),
        Err(unpolled) => return Err(said(unpolled)),
    }
    let now = signed
        .rerunner
        .run(api, (repository, rerun))
        .await
        .map_err(said)?;
    unrerunnable(&now, &planned.head).map_or(Ok(now), Err)
}

/// `action` written to `store` as new, or over a record a later apply may
/// replace, before its call: the version written.
///
/// # Errors
///
/// Where a record of the job says it was asked for, another apply wrote
/// first, or the store could not be read or written, saying why.
async fn marked<A: Actions>(store: &A, action: &RerunRecord) -> Result<Version, String> {
    let expected = match store.action(&action.key).await {
        Ok(None) => None,
        Ok(Some(kept)) if kept.value.retried() => Some(kept.version),
        Ok(Some(kept)) => {
            return Err(format!(
                "herald already asked to re-run it at {}, as {} signed in: {}",
                kept.value.at,
                kept.value.operator,
                kept.value.outcome.said()
            ));
        }
        Err(error) => {
            return Err(format!(
                "herald's record of it could not be read, so nothing was asked: {error}"
            ));
        }
    };
    match store.put_action(action, expected).await {
        Ok(Written::Written(version)) => Ok(version),
        Ok(Written::Conflict) => {
            Err("another apply recorded this job first, so it is not asked for twice".to_owned())
        }
        Err(error) => Err(format!(
            "herald's record of it could not be written, so nothing was asked: {error}"
        )),
    }
}

/// `planned`'s job `rerun` re-run as `signed`, by the operator holding
/// `token`, its record kept in `store`: checked right before the call,
/// recorded before it, its write App's token minted only then, and
/// recorded again with what the forge's answer, or the job read back,
/// says. Where the record cannot be rewritten, it still says the call is
/// being made, and herald never asks for it again.
pub(super) async fn called<S: Store, C: Clock, G: Signer, H: Http, A: Actions>(
    (signed, token, store): (&Signed<'_, H>, &OperatorToken, &A),
    ports: &Ports<'_, S, C, G>,
    (planned, rerun): (&Planned, &Rerun),
) -> Applied {
    let refused = |reason: String| Applied::of(planned, rerun, Done::Refused(reason));
    let repository = planned.repository.as_str();
    let api = &Budgeted::new(signed.api, ports.budget, ports.clock);
    let before = match checked((signed, token), api, (planned, rerun)).await {
        Ok(before) => before,
        Err(reason) => return refused(reason),
    };
    let mut action = RerunRecord {
        key: RunKey {
            api: signed.rerunner.api().to_owned(),
            repository: repository.to_owned(),
            run: rerun.run.clone(),
            job: rerun.job.clone(),
            head: planned.head.clone(),
        },
        operator: signed.operator.clone(),
        credential: signed.rerunner.credential().to_owned(),
        at: ports.clock.now(),
        reruns: before.reruns,
        outcome: Reran::Calling,
    };
    let version = match marked(store, &action).await {
        Ok(version) => version,
        Err(reason) => return refused(reason),
    };
    let credential = match credential(signed, ports, repository).await {
        Ok(credential) => credential,
        Err(reason) => {
            action.outcome = Reran::Unsent;
            return match store.put_action(&action, Some(version)).await {
                Ok(Written::Written(_)) => refused(reason),
                Ok(Written::Conflict) | Err(_) => {
                    Applied::of(planned, rerun, Done::Unrecorded(Reran::Unsent))
                }
            };
        }
    };
    let written = Budgeted::new(&signed.written, ports.budget, ports.clock);
    let answer = signed
        .rerunner
        .rerun(&written, credential, (repository, rerun))
        .await;
    action.outcome = match answer.as_ref().ok().filter(|_| !written.unsent()) {
        None => Reran::Unsent,
        Some(answer) => match answered(answer) {
            Some(outcome) => outcome,
            None => match signed.rerunner.run(api, (repository, rerun)).await {
                Ok(after) if after.reruns > before.reruns || !after.failed => Reran::Rerun,
                _ => Reran::Unsettled,
            },
        },
    };
    let done = match action.outcome {
        Reran::Rerun => Done::Rerun,
        Reran::Refused { status } => Done::Declined(status),
        Reran::Unsettled => Done::Unsettled,
        Reran::Calling | Reran::Unsent => Done::Refused(action.outcome.said().to_owned()),
    };
    match store.put_action(&action, Some(version)).await {
        Ok(Written::Written(_)) => Applied::of(planned, rerun, done),
        _ => Applied::of(planned, rerun, Done::Unrecorded(action.outcome)),
    }
}
