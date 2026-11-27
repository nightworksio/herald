//! herald's own queue: one pull request per branch in flight, brought up to
//! date with its base and merged with the head it proved as the head the
//! forge must find. Right before each step every input it stands on is read
//! again at that head; the step is recorded with compare-and-set before the
//! call, and what came of it after; a pull request that fails leaves at
//! once and comes back only by a new gesture (ADR-0087).

use herald_config::MergeQueue;
use herald_core::Rerun;
use herald_core::merging::{
    GestureRecord, Heads, Leaving, Left, Method, Own, Read, Step, StepKind, Stepped, UPDATE_PAUSE,
    Updated, Verdict, failing, rerun, unvouched, verdict,
};
use herald_kinds::{
    Branched, Done, Entered, Holds, Merge, MergeReader, MergeWriter, Methods, Plan, Proved,
    Unpolled, WriteWith,
};
use herald_ports::{
    Actions, Clock, DynHttp, DynTimer, Erased, Http, Signer, Store, Versioned, Written,
};
use herald_wire::github_app::{merging, rerunning, updating};
use jiff::{SignedDuration, Timestamp};

use super::handing::{Ready, Unfit, ready, ungestured};
use super::left::told;
use super::taking::{after, holding, holds};
use super::{
    Flow, Found, GONE, Handing, Judged, Next, RACED, UNNAMED, UNREAD_AGAIN, again, asked,
    first_record, gesture_key, version,
};
use crate::Ports;
use crate::applying::minted_with;
use crate::budget::Budgeted;

/// Why no step is taken where another gesture stands now.
pub const OWN_REGESTURED: &str = "read again right before the step, the gesture standing on it is another one, so no step was taken; the next run reads it afresh";

/// Why no step is taken where the role of the person who queued it is not
/// known.
pub const OWN_UNSAID: &str = "read again right before the step, the forge would not say the role of the person who queued it, and a role not known is not one lost, so no step was taken; the next run reads it afresh";

/// Why no step is taken where the pull request is a draft now.
pub const OWN_DRAFTED: &str =
    "read again right before the step, it is a draft, so no step was taken";

/// Why no step is taken where reading the pull request again finds it due
/// for another one.
pub const UNDUE: &str = "read again right before the step, it is no longer due for it, so no step was taken; the next run reads it afresh";

/// Why no call is made where the queue's source makes no step on its
/// forge.
const UNWRITABLE: &str = "the queue's source takes no step on its forge, so no call was made";

/// Why a merge is not made where the repository's methods could not be read
/// whole.
pub const METHODS_UNREAD: &str = "the methods the repository allows a merge by could not be read whole, so no merge was made; the source's read credential needs to see the repository's settings";

/// What an apply did with a pull request of herald's own queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acted {
    /// It took the step, which came to `1`.
    Stepped(StepKind, Stepped),
    /// It left the queue for this reason, now recorded.
    Left(Left),
    /// No step was taken, for this reason.
    Refused(String),
    /// The step's call was not sent, for this reason, and is recorded so.
    Unsent(StepKind, String),
    /// The step came to `1`, but herald's record of it could not be
    /// updated, so it still says the call is being made and no step is
    /// taken again for this gesture.
    Unrecorded(StepKind, Stepped),
    /// It left the queue for this reason, recorded for its gesture, but
    /// the record of it a view's row reads could not be written; the next
    /// run that takes the queue's steps writes it while the gesture stands.
    Untold(Left),
}

/// The last update recorded as taken, where one was, on a head the forge
/// said, right before it, merges into the base's head without a conflict:
/// that merge has one content alone, whoever made the commit.
fn updated(own: Option<&Own>) -> Option<&Step> {
    own?.steps
        .iter()
        .rev()
        .find(|step| step.kind == StepKind::Update && step.clean && step.outcome == Stepped::Took)
}

/// Whether the forge itself made and signed `head`, as `queue`'s `signer`
/// says, with exactly `from` and `base` as its parents, in that order.
async fn remade(
    (reader, http): (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    head: &str,
    (from, base): (&str, &str),
) -> Result<bool, Unpolled> {
    let parents = reader
        .forge_made(http, (head, queue.signer.as_deref()))
        .await?;
    Ok(parents.is_some_and(|parents| parents == [from, base]))
}

/// Whether the gesture vouches for `entered`'s head as one herald's own
/// update made from the head it vouched for, as `queue` says: where `own`
/// records that update taken, on a head that merged into the base's head
/// without a conflict, and `entered`'s head is the one herald read right
/// after that update and found the forge itself made and signed with
/// exactly that head and that base's head as its parents, in that order,
/// as the forge still says; or where one who holds the queue's least role
/// approved it; or never.
async fn carried(
    read: (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    own: Option<&Own>,
    entered: &Entered,
) -> Result<bool, Unpolled> {
    let Some(step) = updated(own) else {
        return Ok(false);
    };
    Ok(match queue.updated {
        Updated::Never => false,
        Updated::Approved => entered.approved,
        Updated::Signed => {
            step.made.as_ref() == Some(&entered.head)
                && remade(read, queue, &entered.head, (&step.head, &step.base)).await?
        }
    })
}

/// The head read again through `read` after herald's own update of
/// `fresh` took, until it is another one: read again after each pause
/// `timer` makes while the head has not moved and the pauses come to no
/// more than `queue`'s `update_wait`; none where a read fails, or the head
/// has not moved by then.
async fn moved(
    (read, timer): ((&dyn MergeReader, &dyn DynHttp), &dyn DynTimer),
    queue: &MergeQueue,
    fresh: &Entered,
) -> Option<String> {
    let mut paused = SignedDuration::ZERO;
    loop {
        let head = again(read, queue, fresh.number).await.ok()??.head;
        if head != fresh.head {
            return Some(head);
        }
        paused = paused
            .checked_add(UPDATE_PAUSE)
            .filter(|paused| *paused <= queue.update_wait)?;
        timer.sleep_boxed(UPDATE_PAUSE.unsigned_abs()).await;
    }
}

/// The head herald's own update of `fresh` moved it to, `back` where
/// reading a lost answer back already read it, and else the first other
/// head [`moved`] reads, each read within the run's budget in `ports`,
/// where the queue's `updated` is `signed`, herald can prove a signature
/// on its forge, and the forge itself made and signed that head with
/// exactly `fresh`'s head and `base` as its parents, in that order: the one
/// head the gesture can vouch for after the update; none otherwise, with
/// no read made where it can prove none.
async fn made<S, C: Clock, G>(
    read: (&dyn MergeReader, &dyn DynHttp),
    (timer, ports): (&dyn DynTimer, &Ports<'_, S, C, G>),
    queue: &MergeQueue,
    ((fresh, base), back): ((&Entered, &str), Option<String>),
) -> Option<String> {
    if queue.updated != Updated::Signed
        || !MergeQueue::proves_signed(&queue.forge, queue.signer.as_deref())
    {
        return None;
    }
    let erased = Erased(read.1);
    let budgeted = Budgeted::new(&erased, ports.budget, ports.clock);
    let read = (read.0, &budgeted as &dyn DynHttp);
    let head = match back {
        Some(head) => head,
        None => moved((read, timer), queue, fresh).await?,
    };
    let from = (fresh.head.as_str(), base);
    remade(read, queue, &head, from).await.ok()?.then_some(head)
}

/// Why `entered`'s gesture does not vouch for its head, where it does not,
/// with `seen` the head herald first read with it and `own` what the
/// queue recorded for it: neither by its own record nor carried to a head
/// herald's own update made.
async fn unstanding(
    read: (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    (entered, seen, own): (&Entered, Option<&str>, Option<&Own>),
) -> Result<Option<Left>, Unpolled> {
    let heads = Heads {
        then: entered.gesture.head.as_deref(),
        named: entered.gesture.named.as_deref(),
        seen,
        approved: entered.approved,
    };
    let Some(left) = unvouched(queue.vouches, &heads, &entered.head) else {
        return Ok(None);
    };
    Ok((!carried(read, queue, own, entered).await?).then_some(left))
}

/// The step a pull request read again right before it is due for, with
/// what it takes.
#[derive(Debug)]
enum Due {
    /// Its branch brought up to date.
    Update,
    /// It merged by the method.
    Merge(Method),
    /// The failed job of the check re-run.
    Rerun(String, Rerun),
}

/// The step `verdict` says is due, where it says one is.
const fn stepping(verdict: Verdict) -> Option<StepKind> {
    match verdict {
        Verdict::Update => Some(StepKind::Update),
        Verdict::Merge => Some(StepKind::Merge),
        Verdict::Rerun => Some(StepKind::Rerun),
        Verdict::Leaves(_) | Verdict::Wait => None,
    }
}

/// The failed check on the head `read` proves whose job the queue's
/// `retries` re-runs next, as `own` records the re-runs sent.
fn rerun_due<'r>(queue: &MergeQueue, read: &Read<'r>, own: Option<&Own>) -> Option<&'r str> {
    let failing = failing(read, &queue.may_fail);
    rerun(queue.retries.as_ref(), &failing, own, read.head)
}

/// What comes next for the pull request `read` proves, a failed check
/// re-run first where `rerun` names one.
fn judged(queue: &MergeQueue, read: &Read<'_>, rerun: Option<&str>) -> Verdict {
    match verdict(read, &queue.may_fail, queue.wait) {
        Verdict::Leaves(Left::ChecksFailed) if rerun.is_some() => Verdict::Rerun,
        verdict => verdict,
    }
}

/// What `proved` proves of `entered`, vouched for, as of `now`.
fn read_of<'a>(
    queue: &MergeQueue,
    entered: &'a Entered,
    proved: &'a Proved,
    now: Timestamp,
) -> Read<'a> {
    let since = entered.arrived.map_or(entered.gesture.at, |arrived| {
        arrived.max(entered.gesture.at)
    });
    Read {
        entered: true,
        vouches: queue.vouches,
        heads: Heads {
            named: Some(&entered.head),
            ..Heads::default()
        },
        head: &entered.head,
        up_to_date: proved.up_to_date,
        clean: proved.clean,
        checks: &proved.checks,
        required: &proved.required,
        mergeable: proved.mergeable,
        waited: now.duration_since(since),
        whole: proved.whole,
    }
}

/// What herald's own queue does next with `entered`, on the base
/// `branched`, as of `now`, with `seen` the head herald first read with its
/// gesture and `kept` its record: the pull request in flight is proved
/// through `read`, and `flying` says whether one already is.
///
/// # Errors
///
/// Where proving it could not be read.
pub(super) async fn next(
    read: (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    (entered, seen, kept): (&Entered, Option<&str>, Option<&Versioned<GestureRecord>>),
    branched: &Branched,
    (now, flying): (Timestamp, &mut bool),
) -> Result<Next, Unpolled> {
    let own = kept.map(|kept| &kept.value.own);
    if let Some(leaving) = own.and_then(|own| own.left) {
        return Ok(Next::Verdict(Verdict::Leaves(leaving.left)));
    }
    if let Some(left) = unstanding(read, queue, (entered, seen, own)).await? {
        return Ok(Next::Verdict(Verdict::Leaves(left)));
    }
    if *flying {
        return Ok(Next::Behind);
    }
    *flying = true;
    let unknown = own.and_then(|own| own.steps.iter().rev().find(|step| step.outcome.unknown()));
    if let Some(step) = unknown {
        return Ok(Next::Held(step.clone()));
    }
    let proved = read.0.proved(read.1, entered, branched).await?;
    let proving = read_of(queue, entered, &proved, now);
    let verdict = judged(queue, &proving, rerun_due(queue, &proving, own));
    let Some(kind) = stepping(verdict) else {
        *flying = !matches!(verdict, Verdict::Leaves(_));
        return Ok(Next::Verdict(verdict));
    };
    let held = own.and_then(|own| own.holding(kind, &entered.head));
    Ok(match held {
        Some(step) if now.duration_since(step.at) >= queue.wait => {
            *flying = false;
            Next::Verdict(Verdict::Leaves(Left::CannotUpdate))
        }
        Some(step) => Next::Held(step.clone()),
        None => Next::Verdict(verdict),
    })
}

/// `method` as the repository allows it, `methods` being the ones it
/// allows: `repository` taken as its one allowed method.
///
/// # Errors
///
/// Where the queue names no method, the methods were not read whole, or
/// the repository does not allow it, or allows more than one where it is
/// `repository`.
pub fn allowed(method: Option<Method>, methods: &Methods) -> Result<Method, String> {
    let method = method.ok_or(super::METHODLESS)?;
    if !methods.whole {
        return Err(METHODS_UNREAD.to_owned());
    }
    let names = || {
        methods
            .allowed
            .iter()
            .map(|method| method.name())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (method, methods.allowed.as_slice()) {
        (Method::Repository, [one]) => Ok(*one),
        (Method::Repository, _) => Err(format!(
            "method = \"repository\" takes the repository's one allowed method, and it allows {}; name one in method",
            names()
        )),
        (method, allowed) if allowed.contains(&method) => Ok(method),
        (method, _) => Err(format!(
            "the repository does not allow a merge by \"{}\"; it allows {}",
            method.name(),
            names()
        )),
    }
}

/// What a step came to: its outcome, the head it made that the gesture
/// can vouch for, and why its call was not sent, where it says.
type Settled = (Stepped, Option<String>, Option<String>);

/// `record` with its last step come to `outcome` as of `now`, with the
/// head it `made` where it made one the gesture can vouch for, and why the
/// pull request leaves where it does, written to `store` over its version:
/// what the apply did, with `why` the call was not sent where it says.
async fn settled<A: Actions>(
    store: &A,
    (mut record, at): (GestureRecord, herald_ports::Version),
    (kind, (outcome, made, why), now): (StepKind, Settled, Timestamp),
) -> Next {
    if let Some(step) = record.own.steps.last_mut() {
        step.outcome = outcome;
        step.made = made;
    }
    let leaving = outcome.left(kind).map(|left| Leaving { left, at: now });
    record.own.left = leaving.or(record.own.left);
    Next::Acted(match (store.put_gesture(&record, Some(at)).await, why) {
        (Ok(Written::Written(_)), Some(why)) => Acted::Unsent(kind, why),
        (Ok(Written::Written(_)), None) => match leaving {
            Some(leaving) if !told(store, &record.key, leaving).await => {
                Acted::Untold(leaving.left)
            }
            _ => Acted::Stepped(kind, outcome),
        },
        (Ok(Written::Conflict) | Err(_), _) => Acted::Unrecorded(kind, outcome),
    })
}

/// The record of `entered`'s gesture in the queue `named`, `kept` where
/// one is, a new one first seen `now` where none is; none where the forge
/// named no id for the gesture.
fn record_of(
    (named, queue): (&str, &MergeQueue),
    entered: &Entered,
    kept: Option<&Versioned<GestureRecord>>,
    now: Timestamp,
) -> Option<GestureRecord> {
    if let Some(kept) = kept {
        return Some(kept.value.clone());
    }
    Some(first_record(
        gesture_key(queue, entered)?,
        named,
        entered,
        now,
    ))
}

/// `entered`'s leaving for `left` as of `now` recorded in `store` over
/// `kept`: what the apply did, or the verdict alone where it could not be
/// recorded and the next run judges it again.
async fn leaving<A: Actions>(
    store: &A,
    queued: (&str, &MergeQueue),
    (entered, kept): (&Entered, Option<&Versioned<GestureRecord>>),
    (left, now): (Left, Timestamp),
) -> Next {
    let Some(mut record) = record_of(queued, entered, kept, now) else {
        return Next::Verdict(Verdict::Leaves(left));
    };
    let leaving = Leaving { left, at: now };
    record.own.left = Some(leaving);
    match store.put_gesture(&record, version(kept)).await {
        Ok(Written::Written(_)) if told(store, &record.key, leaving).await => {
            Next::Acted(Acted::Left(left))
        }
        Ok(Written::Written(_)) => Next::Acted(Acted::Untold(left)),
        _ => Next::Verdict(Verdict::Leaves(left)),
    }
}

/// What reading `fresh` back after the forge answered the step `due`
/// with `done` says came of it, with the head read back where an update's
/// answer was lost and the head moved.
async fn answered(
    (reader, http): (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    (fresh, due): (&Entered, &Due),
    done: Done,
) -> (Stepped, Option<String>) {
    let back = || again((reader, http), queue, fresh.number);
    let stepped = match done {
        Done::Took => Stepped::Took,
        Done::Conflict(_) => Stepped::Conflict,
        Done::Refused(_) => match back().await {
            Ok(Some(back)) if back.head != fresh.head => Stepped::HeadMoved,
            _ => Stepped::Refused,
        },
        Done::Lost => match due {
            Due::Merge(_) => match reader.merged(http, fresh.number).await {
                Ok(true) => Stepped::Took,
                _ => Stepped::Unsettled,
            },
            Due::Rerun(check, job) => match reader.failed(http, (&fresh.head, check)).await {
                Ok(Plan::Runs(runs)) if runs.iter().any(|run| run.job == job.job) => {
                    Stepped::Unsettled
                }
                Ok(_) => Stepped::Took,
                Err(_) => Stepped::Unsettled,
            },
            Due::Update => {
                return match back().await {
                    Ok(Some(back)) if back.head != fresh.head => (Stepped::Took, Some(back.head)),
                    _ => (Stepped::Unsettled, None),
                };
            }
        },
    };
    (stepped, None)
}

/// What reading the pull request again right before a step of `kind` on
/// `entered` finds, on a fresh read of its base: the pull request, its
/// base and what the step takes where every input stands at its head and
/// it is due for that step, or what the apply does instead.
async fn fresh_read(
    read: (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    (entered, kept, kind): (&Entered, Option<&Versioned<GestureRecord>>, StepKind),
    now: Timestamp,
) -> Result<(Entered, Branched, Proved, Due), Result<Left, String>> {
    let (reader, http) = read;
    let unread = |error: Unpolled| Err(unread_again(error));
    let fresh = match again(read, queue, entered.number).await {
        Ok(fresh) => fresh,
        Err(error) => return Err(unread(error)),
    };
    let standing = |fresh: &Entered| ungestured(queue, entered, fresh) != Some(Unfit::Unroled);
    let Some(fresh) = fresh.filter(standing) else {
        return Err(unstood(read, queue, entered).await);
    };
    match ungestured(queue, entered, &fresh) {
        Some(Unfit::Unread) => return Err(Err(UNREAD_AGAIN.to_owned())),
        Some(Unfit::Regestured) => return Err(Err(OWN_REGESTURED.to_owned())),
        Some(Unfit::Drafted) => return Err(Err(OWN_DRAFTED.to_owned())),
        Some(Unfit::Unroled) | None => {}
    }
    if fresh.head != entered.head {
        return Err(Ok(Left::HeadMoved));
    }
    let own = kept.map(|kept| &kept.value.own);
    let seen = kept.map(|kept| kept.value.seen.as_str());
    match unstanding(read, queue, (&fresh, seen, own)).await {
        Ok(Some(left)) => return Err(Ok(left)),
        Ok(None) => {}
        Err(error) => return Err(unread(error)),
    }
    let branched = match reader.branch(http, &queue.branch).await {
        Ok(branched) if branched.whole => branched,
        Ok(_) => return Err(Err(UNREAD_AGAIN.to_owned())),
        Err(error) => return Err(unread(error)),
    };
    let proved = match reader.proved(http, &fresh, &branched).await {
        Ok(proved) => proved,
        Err(error) => return Err(unread(error)),
    };
    let proving = read_of(queue, &fresh, &proved, now);
    let rerunning = rerun_due(queue, &proving, own);
    let due = match (judged(queue, &proving, rerunning), kind, rerunning) {
        (Verdict::Leaves(left), _, _) => return Err(Ok(left)),
        (Verdict::Update, StepKind::Update, _) => Due::Update,
        (Verdict::Merge, StepKind::Merge, _) => {
            let methods = reader.methods(http).await.map_err(unread)?;
            Due::Merge(allowed(queue.method, &methods).map_err(Err)?)
        }
        (Verdict::Rerun, StepKind::Rerun, Some(check)) => {
            let plan = reader.failed(http, (&fresh.head, check)).await;
            let job = match plan.map_err(unread)? {
                Plan::Runs(runs) => runs.into_iter().next(),
                Plan::Unplanned(_) => None,
            };
            let Some(job) = job else {
                return Err(Ok(Left::ChecksFailed));
            };
            Due::Rerun(check.to_owned(), job)
        }
        _ => return Err(Err(UNDUE.to_owned())),
    };
    Ok((fresh, branched, proved, due))
}

/// Why no step is taken where the pull request could not be read again,
/// as `error` says.
fn unread_again(error: Unpolled) -> String {
    format!(
        "it could not be read again right before the step, so no step was taken: {}",
        error.into_reason()
    )
}

/// What `entered` comes to where its gesture no longer stands, read again
/// through `read` with every mark on it whoever made it. A newer gesture,
/// or a cancel made after it, by one who holds the queue's least role now
/// takes no step; marks by anyone else count for nothing. Else it leaves
/// as removed where the gesture's maker holds a role below the least or
/// none, as for an account gone; where the forge would not say, or the
/// marks were not read whole, no step is taken, and why.
async fn unstood(
    (reader, http): (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    entered: &Entered,
) -> Result<Left, String> {
    let pull = (queue.branch.as_str(), entered.number);
    let taken = match reader.took_back(http, pull, asked(queue)).await {
        Ok(Some(taken)) if taken.whole => taken,
        Ok(Some(_)) => return Err(UNREAD_AGAIN.to_owned()),
        Ok(None) => return Err(GONE.to_owned()),
        Err(error) => return Err(unread_again(error)),
    };
    let gesture = &entered.gesture;
    let Some((at, anchor)) = taken
        .gestures
        .iter()
        .enumerate()
        .find(|(_, made)| made.id == gesture.id && made.at == gesture.at)
    else {
        return Err(GONE.to_owned());
    };
    let newer = taken.gestures.get(..at).unwrap_or_default();
    let mut since = newer.iter().chain(
        taken
            .cancels
            .iter()
            .filter(|cancel| after(cancel, anchor, queue.skew)),
    );
    if since.any(|mark| holding(queue, &taken, mark)) {
        return Err(GONE.to_owned());
    }
    match holds(&taken, anchor) {
        Some(Holds::Role(role)) if role >= queue.least => Err(GONE.to_owned()),
        Some(Holds::Role(_) | Holds::Nothing) => Ok(Left::Removed),
        Some(Holds::Unsaid) | None => Err(OWN_UNSAID.to_owned()),
    }
}

/// The pull request `entered`, judged due for a step of `kind` in the queue
/// `named`, `queue`, with its record `kept`: read again through `read` and
/// checked, the step recorded in `store` first, its write App's token
/// minted only then for that step alone, the call made naming the head the
/// forge must find where the forge takes one and otherwise right after its
/// head is read again, and what came of it recorded, with the head an
/// update moved it to read after the pauses `timer` makes.
async fn step<S: Store, C: Clock, G: Signer, H: Http, A: Actions>(
    (ports, store, ready, timer): (&Ports<'_, S, C, G>, &A, &Ready<H>, &dyn DynTimer),
    read: (&dyn MergeReader, &dyn DynHttp),
    (named, queue): (&str, &MergeQueue),
    (entered, kept, kind): (&Entered, Option<&Versioned<GestureRecord>>, StepKind),
) -> Next {
    let now = ports.clock.now();
    let (fresh, branched, proved, due) =
        match fresh_read(read, queue, (entered, kept, kind), now).await {
            Ok(fresh) => fresh,
            Err(Ok(left)) => {
                return leaving(store, (named, queue), (entered, kept), (left, now)).await;
            }
            Err(Err(why)) => return Next::Acted(Acted::Refused(why)),
        };
    let Some(mut record) = record_of((named, queue), &fresh, kept, now) else {
        return Next::Acted(Acted::Refused(UNNAMED.to_owned()));
    };
    record.own.steps.push(Step {
        kind,
        head: fresh.head.clone(),
        base: branched.head.clone(),
        clean: proved.clean == Some(true),
        made: None,
        check: match &due {
            Due::Rerun(check, _) => Some(check.clone()),
            Due::Update | Due::Merge(_) => None,
        },
        credential: ready.credential.clone(),
        at: now,
        outcome: Stepped::Calling,
    });
    let at = match store.put_gesture(&record, version(kept)).await {
        Ok(Written::Written(at)) => at,
        Ok(Written::Conflict) => return Next::Acted(Acted::Refused(RACED.to_owned())),
        Err(error) => {
            return Next::Acted(Acted::Refused(format!(
                "herald's record of it could not be written, so no step was taken: {error}"
            )));
        }
    };
    let narrowing = match kind {
        StepKind::Update => updating as fn(&str) -> _,
        StepKind::Merge => merging as fn(&str) -> _,
        StepKind::Rerun => rerunning as fn(&str) -> _,
    };
    let narrowed = (queue.forge.repository.as_str(), narrowing);
    let minted = minted_with((&ready.writing, &ready.written), ports, narrowed).await;
    let writer = minted.and_then(|credential| {
        let with = WriteWith {
            merging: &queue.forge,
            credential,
        };
        ready.source.merge_writer(with).ok_or(UNWRITABLE.to_owned())
    });
    let writer = match writer {
        Ok(writer) => writer,
        Err(why) => {
            let unsent = (Stepped::Unsent, None, Some(why));
            return settled(store, (record, at), (kind, unsent, now)).await;
        }
    };
    let (outcome, back) = called(
        ports,
        (read, &*writer, &ready.written),
        queue,
        (&fresh, &due),
    )
    .await;
    let made = match (outcome, &due) {
        (Stepped::Took, Due::Update) => {
            let stepped = ((&fresh, branched.head.as_str()), back);
            made(read, (timer, ports), queue, stepped).await
        }
        _ => None,
    };
    settled(store, (record, at), (kind, (outcome, made, None), now)).await
}

/// The step `due` on `fresh` made through `writer` over `written` within
/// the run's budget: what came of it, its answer read back through `read`
/// where it says neither, with the head read back where it was.
async fn called<S: Store, C: Clock, G: Signer, H: Http>(
    ports: &Ports<'_, S, C, G>,
    (read, writer, written): ((&dyn MergeReader, &dyn DynHttp), &dyn MergeWriter, &H),
    queue: &MergeQueue,
    (fresh, due): (&Entered, &Due),
) -> (Stepped, Option<String>) {
    if matches!(due, Due::Update) && !writer.pins_update() {
        let current = again(read, queue, fresh.number).await;
        if !matches!(&current, Ok(Some(current)) if current.head == fresh.head) {
            return (Stepped::Unsent, None);
        }
    }
    let written = Budgeted::new(written, ports.budget, ports.clock);
    let answer = match due {
        Due::Merge(method) => {
            let merge = Merge {
                method: *method,
                message: queue.message,
                entered: fresh,
            };
            writer.merge(&written, merge).await
        }
        Due::Rerun(_, job) => writer.rerun(&written, job).await,
        Due::Update => writer.update(&written, fresh, queue.forge.update).await,
    };
    if written.unsent() {
        return (Stepped::Unsent, None);
    }
    answered(read, queue, (fresh, due), answer.unwrap_or(Done::Lost)).await
}

/// `judged`, the entries of herald's own queue `named`, `queue`: the one in
/// flight brought up to date or merged with the write credential `flow`
/// and `handing` make ready, and each one leaving recorded, its record
/// kept in `store`.
pub(super) async fn stepped<S: Store, C: Clock, G: Signer, H: Http, A: Actions>(
    (flow, handing): (Flow<'_, '_, S, C, G, H>, &Handing<'_, H>),
    store: &A,
    read: (&dyn MergeReader, &dyn DynHttp),
    (named, queue): (&str, &MergeQueue),
    judged: Vec<(Judged, Option<Versioned<GestureRecord>>)>,
) -> Found {
    let due = judged.iter().any(
        |(judged, _)| matches!(judged.next, Next::Verdict(verdict) if stepping(verdict).is_some()),
    );
    let prepared = if due {
        Some(ready(flow, handing, queue).await)
    } else {
        None
    };
    let now = flow.2.clock.now();
    let mut entries = Vec::with_capacity(judged.len());
    for (judged, kept) in judged {
        let kind = match &judged.next {
            Next::Verdict(verdict) => stepping(*verdict),
            _ => None,
        };
        let next = match (&judged.next, kind, &prepared) {
            (Next::Verdict(Verdict::Leaves(left)), _, _) => {
                let recorded = kept.as_ref().and_then(|kept| {
                    let leaving = kept.value.own.left?;
                    Some((&kept.value.key, leaving))
                });
                match recorded {
                    None => {
                        let gestured = (&judged.entered, kept.as_ref());
                        leaving(store, (named, queue), gestured, (*left, now)).await
                    }
                    Some((key, kept_left)) if told(store, key, kept_left).await => {
                        judged.next.clone()
                    }
                    Some(_) => Next::Acted(Acted::Untold(*left)),
                }
            }
            (_, Some(kind), Some(Ok(ready))) => {
                let gestured = (&judged.entered, kept.as_ref(), kind);
                let stepping = (flow.2, store, ready, handing.timer);
                step(stepping, read, (named, queue), gestured).await
            }
            (_, Some(_), Some(Err(why))) => Next::Acted(Acted::Refused(why.clone())),
            _ => judged.next.clone(),
        };
        entries.push(Judged { next, ..judged });
    }
    Found::Entries(entries)
}

#[cfg(test)]
mod tests;
