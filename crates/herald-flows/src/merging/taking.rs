//! The take-out: a pull request herald handed to the forge's queue taken
//! out of it again, with the queue's own write credential, judged against
//! herald's record of what it handed over: the newest gesture on it with a
//! reached hand-off record. Against that gesture, a newer gesture by one
//! who holds the queue's least role now, made after any cancel that
//! counts, stands; else the newest cancel made after it by one who holds
//! that role now takes it out; else it is taken out where the gesture's
//! maker no longer holds that role. Right before the call, the pull request
//! is read again on its own and judged the same way; the record is written
//! before the call, and what came of it after (ADR-0087).

use std::sync::Arc;

use herald_config::MergeQueue;
use herald_core::merging::{CancelKey, GestureKey, GestureRecord, RoleLost, TakeOut, TakenOut};
use herald_kinds::{Answered, Holds, Marked, MergeReader, TakenBack, WriteWith};
use herald_ports::{Actions, Clock, DynHttp, Http, Signer, Store, Version, Versioned, Written};
use herald_wire::github_app::handing_off;
use jiff::SignedDuration;

use super::handing::Ready;
use super::{Cancel, TookOut, Undo, asked};
use crate::Ports;
use crate::applying::minted_with;
use crate::budget::Budgeted;

/// Why a pull request is not taken out where its marks were not read
/// whole.
pub const UNREAD_CANCEL: &str = "what decides whether it is taken out of the forge's queue was not read whole, so herald did not take it out";

/// Why a cancel is not carried out where the forge named no id for it.
pub const UNNAMED_CANCEL: &str = "the forge named no id for the cancel, so herald can keep no record of it and did not take it out of the forge's queue";

/// Why a cancel is not carried out where its maker holds no role at or
/// above the queue's least now.
pub const UNROLED_CANCEL: &str = "the person who took it back does not hold the queue's least role now, so herald did not take it out of the forge's queue; one who does may take it back again";

/// Why a pull request is not taken out where reading it again finds
/// nothing that takes it out.
pub const UNTAKEN: &str = "read again right before the step, nothing takes it out of the forge's queue any more (a newer gesture by one who holds the queue's least role stands, or no such person's cancel stands after the gesture herald handed over and its maker holds that role) or it is no longer open into the branch, so herald did not take it out";

/// Why a pull request is not taken out for the role lost where the forge
/// would not say the role of the person whose gesture herald handed over.
pub const UNSAID_ROLE: &str = "the forge would not say the role of the person whose gesture herald handed over, and a role not known is not one lost, so herald did not take it out of the forge's queue";

/// Why a pull request is not taken out where what takes it out changed.
pub const RECANCELLED: &str = "read again right before the step, the cancel that takes it out is another one, or one does where none did or none does where one did, so herald did not take it out of the forge's queue; the next run reads it afresh";

/// Why a pull request is not taken out where the gesture herald handed over
/// is no longer recorded on it.
pub const UNHANDED: &str = "read again right before the step, the gesture herald handed over is no longer recorded on it, so herald did not take it out of the forge's queue";

/// Why a pull request is not taken out where herald's record of a gesture
/// made on it names another time for the gesture's id than the forge does.
pub const UNMATCHED_OUT: &str = "herald's record of a gesture made on it names another time for the gesture's id than the forge does now, so it is not the same gesture and herald did not take it out of the forge's queue";

/// Why a pull request is not taken out where another run recorded taking
/// it out first.
pub const RACED_OUT: &str = "another run recorded taking it out of the forge's queue for this cancel or the role lost first, so it is not asked twice";

/// What takes a pull request out of the forge's queue, or keeps it there.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Carried {
    /// The cancel with this key takes it out, or the role lost where none.
    Out(Option<CancelKey>),
    /// The role is lost, and the queue's `role_lost = "stay"` leaves it in.
    Stays,
    /// It stays in the forge's queue, for this reason.
    Kept(&'static str),
    /// Nothing takes it out: it stays, with nothing to say.
    Stands,
}

impl Carried {
    /// The cancel that takes it out, or none for the role lost; or what
    /// the queue does with it instead.
    fn out(self) -> Result<Option<CancelKey>, Undo> {
        match self {
            Self::Out(cancel) => Ok(cancel),
            Self::Stays => Err(Undo::Stays),
            Self::Kept(why) => Err(Undo::Uncarried(why.to_owned())),
            Self::Stands => Err(Undo::Uncarried(UNTAKEN.to_owned())),
        }
    }
}

/// Whether `cancel` counts as made after `gesture`: stamped no more than
/// `skew` before it.
pub(super) fn after(cancel: &Marked, gesture: &Marked, skew: SignedDuration) -> bool {
    gesture.at.duration_since(cancel.at) <= skew
}

/// Whether the person who made `mark` holds `queue`'s least role now, as
/// `taken` reads it.
pub(super) fn holding(queue: &MergeQueue, taken: &TakenBack, mark: &Marked) -> bool {
    taken
        .role_of(&mark.by)
        .is_some_and(|role| role >= queue.least)
}

/// What `queue` does with `taken`, judged against `handed`, the gesture
/// herald handed over, and the gestures `newer` than it; with the cancel
/// it names. A newer gesture by one who holds the least role now, made
/// after any cancel that counts, stands. Else the newest cancel made after the anchor by one who holds
/// the least role now takes it out where it is named. Else, where the
/// anchor's maker holds a role below the least or none, it is taken out
/// for the role lost, or stays where the queue says so; where the maker
/// holds the least role, the newest cancel after the anchor is shown as
/// one herald could not carry out; where the forge would not say, it stays
/// and is shown.
fn carried<'t>(
    queue: &MergeQueue,
    taken: &'t TakenBack,
    (newer, handed): Anchor<'t>,
) -> (Carried, Option<&'t Marked>) {
    if !taken.whole {
        return (Carried::Kept(UNREAD_CANCEL), None);
    }
    let mut since = taken
        .cancels
        .iter()
        .filter(|cancel| after(cancel, handed, queue.skew));
    let newest = since.clone().next();
    let cancel = since.find(|cancel| holding(queue, taken, cancel));
    let regestured = newer.iter().any(|gesture| {
        holding(queue, taken, gesture)
            && cancel.is_none_or(|cancel| !after(cancel, gesture, queue.skew))
    });
    if regestured {
        return (Carried::Stands, None);
    }
    let uncarried = match cancel {
        Some(Marked {
            id: Some(id),
            by,
            at,
        }) => {
            let key = CancelKey {
                id: *id,
                by: by.clone(),
                at: *at,
            };
            return (Carried::Out(Some(key)), cancel);
        }
        Some(_) => Some(UNNAMED_CANCEL),
        None => newest.map(|_| UNROLED_CANCEL),
    };
    let shown = cancel.or(newest);
    match holds(taken, handed) {
        Some(Holds::Role(role)) if role >= queue.least => {
            (uncarried.map_or(Carried::Stands, Carried::Kept), shown)
        }
        Some(Holds::Role(_) | Holds::Nothing) if queue.role_lost == RoleLost::Stay => {
            (Carried::Stays, None)
        }
        Some(Holds::Role(_) | Holds::Nothing) => (Carried::Out(None), None),
        Some(Holds::Unsaid) | None => (Carried::Kept(uncarried.unwrap_or(UNSAID_ROLE)), shown),
    }
}

/// A gesture on a pull request, with every gesture made on it after,
/// newest first.
type Anchor<'t> = (&'t [Marked], &'t Marked);

/// `gesture`, at `at` among `taken`'s gestures, with those newer than it.
fn anchored<'t>(taken: &'t TakenBack, (at, gesture): (usize, &'t Marked)) -> Anchor<'t> {
    (taken.gestures.get(..at).unwrap_or_default(), gesture)
}

/// The gesture `record` is of among `taken`'s: the one the forge names by
/// its id at the time herald recorded.
fn made<'t>(taken: &'t TakenBack, record: &GestureRecord) -> Option<Anchor<'t>> {
    taken
        .gestures
        .iter()
        .enumerate()
        .find(|(_, gesture)| gesture.id == Some(record.key.gesture) && gesture.at == record.at)
        .map(|found| anchored(taken, found))
}

/// The role the person who made `gesture` holds now, as `taken` reads it:
/// none where the forge names no one for it, as for an account gone; not
/// known where the forge would not say.
pub(super) fn holds(taken: &TakenBack, gesture: &Marked) -> Option<Holds> {
    if gesture.by.is_empty() {
        return Some(Holds::Nothing);
    }
    taken.makers.get(&gesture.by).copied()
}

/// Who herald recorded made the gesture `record` is of, where the forge
/// names someone else for it on `taken` now, as for an account gone.
fn renamed(taken: &TakenBack, record: &GestureRecord) -> Option<String> {
    made(taken, record)
        .filter(|(_, gesture)| gesture.by != record.by)
        .map(|_| record.by.clone())
}

/// The record in `store`, where one keeps records, of the newest gesture
/// made on `taken` whose hand-off may have put it in the forge's queue, in
/// `queue`, with that gesture; a gesture the forge named no id for is
/// passed over.
async fn handed<'t, A: Actions>(
    store: Option<&A>,
    queue: &MergeQueue,
    taken: &'t TakenBack,
) -> Result<Option<(Anchor<'t>, Versioned<GestureRecord>)>, String> {
    let Some(store) = store else { return Ok(None) };
    for (at, gesture) in taken.gestures.iter().enumerate() {
        let Some(id) = gesture.id else {
            continue;
        };
        let key = GestureKey {
            api: queue.forge.api.clone(),
            repository: queue.forge.repository.clone(),
            number: taken.number,
            gesture: id,
        };
        let kept = store.gesture(&key).await.map_err(|error| {
            format!("herald's record of the gesture could not be read, so herald did not take it out of the forge's queue: {error}")
        })?;
        if kept
            .as_ref()
            .is_some_and(|kept| kept.value.at != gesture.at)
        {
            return Err(UNMATCHED_OUT.to_owned());
        }
        if let Some(kept) = kept.filter(|kept| kept.value.reached()) {
            return Ok(Some((anchored(taken, (at, gesture)), kept)));
        }
    }
    Ok(None)
}

/// What `queue` does with `taken`, as read with the queue, its records in
/// `store` where it keeps them; with the record of the gesture herald
/// handed over. None where herald recorded no hand-off of it, or nothing
/// takes it out: nothing to say.
pub(super) async fn undone<A: Actions>(
    store: Option<&A>,
    queue: &MergeQueue,
    taken: TakenBack,
) -> Option<(Cancel, Option<Versioned<GestureRecord>>)> {
    if !taken.whole {
        let next = Undo::Uncarried(UNREAD_CANCEL.to_owned());
        return Some((cancelled(taken, next), None));
    }
    let (anchor, kept) = match handed(store, queue, &taken).await {
        Ok(Some(found)) => found,
        Ok(None) => return None,
        Err(why) => return Some((cancelled(taken, Undo::Uncarried(why)), None)),
    };
    let (verdict, shown) = carried(queue, &taken, anchor);
    let shown = shown.cloned();
    let next = match verdict {
        Carried::Stands => return None,
        verdict => match verdict.out() {
            Ok(cancel) => match kept.value.taken_out_for(cancel.as_ref()) {
                Some(out) if out.outcome != TakenOut::Unsent => Undo::Recorded {
                    outcome: out.outcome,
                    at: out.at,
                },
                _ => Undo::TakeOut,
            },
            Err(undo) => undo,
        },
    };
    let renamed = renamed(&taken, &kept.value);
    Some((
        Cancel {
            taken,
            cancel: shown,
            next,
            renamed,
        },
        Some(kept),
    ))
}

/// `taken`, judged against no gesture, coming to `next`.
const fn cancelled(taken: TakenBack, next: Undo) -> Cancel {
    Cancel {
        taken,
        cancel: None,
        next,
        renamed: None,
    }
}

/// What this apply did for `outcome`, the forge having said `said` where
/// it refused, and herald `why` where nothing was sent.
fn took(outcome: TakenOut, said: Option<String>, why: Option<String>) -> Undo {
    match outcome {
        TakenOut::Out => Undo::Took(TookOut::Out),
        TakenOut::Refused => Undo::Took(TookOut::Declined(said.unwrap_or_default())),
        TakenOut::Unsettled => Undo::Took(TookOut::Unsettled),
        TakenOut::Calling | TakenOut::Unsent => {
            Undo::Uncarried(why.unwrap_or_else(|| outcome.said().to_owned()))
        }
    }
}

/// `record` written to `store` with its last take-out come to `outcome`,
/// over the version `at`: what this apply did, or that it could not be
/// recorded.
async fn settled<A: Actions>(
    store: &A,
    (mut record, at): (GestureRecord, Version),
    (outcome, said, why): (TakenOut, Option<String>, Option<String>),
) -> Undo {
    if let Some(out) = record.taken_out.last_mut() {
        out.outcome = outcome;
    }
    match store.put_gesture(&record, Some(at)).await {
        Ok(Written::Written(_)) => took(outcome, said, why),
        Ok(Written::Conflict) | Err(_) => Undo::Took(TookOut::Unrecorded(outcome)),
    }
}

/// Whether reading pull request `number` back finds it out of the forge's
/// queue; `otherwise` where it is still there or cannot be read.
async fn back(
    (reader, http): (&dyn MergeReader, &dyn DynHttp),
    number: u64,
    otherwise: TakenOut,
) -> TakenOut {
    match reader.queued(http, number).await {
        Ok(false) => TakenOut::Out,
        _ => otherwise,
    }
}

/// `cancel`, read with the queue `queue`, taken out of the forge's queue
/// as `ready` makes it, the record `kept` of the gesture herald handed
/// over: read again through `reader` and `http` and judged again against
/// that gesture, recorded in `store`, its write App's token minted only
/// then, the call made, and what came of it recorded. A lost answer is
/// read back, never asked again.
pub(super) async fn taken_out<S: Store, C: Clock, G: Signer, H: Http, A: Actions>(
    (ports, store, ready): (&Ports<'_, S, C, G>, &A, &Ready<H>),
    read: (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    (judged, kept): (&Cancel, Versioned<GestureRecord>),
) -> Undo {
    let (reader, http) = read;
    let fresh = match reader
        .took_back(http, (&queue.branch, judged.taken.number), asked(queue))
        .await
    {
        Ok(Some(fresh)) => fresh,
        Ok(None) => return Undo::Uncarried(UNTAKEN.to_owned()),
        Err(error) => {
            return Undo::Uncarried(format!(
                "it could not be read again right before the step, so herald did not take it out of the forge's queue: {}",
                error.into_reason()
            ));
        }
    };
    let Some(anchor) = made(&fresh, &kept.value) else {
        return Undo::Uncarried(UNHANDED.to_owned());
    };
    let (verdict, shown) = carried(queue, &fresh, anchor);
    let cancel = match verdict.out() {
        Ok(cancel) => cancel,
        Err(undo) => return undo,
    };
    if shown != judged.cancel.as_ref() {
        return Undo::Uncarried(RECANCELLED.to_owned());
    }
    let mut record = kept.value;
    record.taken_out.retain(|out| out.cancel != cancel);
    record.taken_out.push(TakeOut {
        cancel,
        credential: ready.credential.clone(),
        at: ports.clock.now(),
        outcome: TakenOut::Calling,
    });
    let at = match store.put_gesture(&record, Some(kept.version)).await {
        Ok(Written::Written(at)) => at,
        Ok(Written::Conflict) => return Undo::Uncarried(RACED_OUT.to_owned()),
        Err(error) => {
            return Undo::Uncarried(format!(
                "herald's record of it could not be written, so herald did not take it out of the forge's queue: {error}"
            ));
        }
    };
    let narrowing = (
        queue.forge.repository.as_str(),
        handing_off as fn(&str) -> _,
    );
    let minted = minted_with((&ready.writing, &ready.written), ports, narrowing).await;
    let writer = minted.and_then(|credential| {
        let with = WriteWith {
            merging: &queue.forge,
            credential,
        };
        ready
            .source
            .merge_writer(with)
            .ok_or(super::NO_FORGE_QUEUE.to_owned())
    });
    let writer: Arc<dyn herald_kinds::MergeWriter> = match writer {
        Ok(writer) => writer,
        Err(why) => {
            return settled(store, (record, at), (TakenOut::Unsent, None, Some(why))).await;
        }
    };
    let written = Budgeted::new(&ready.written, ports.budget, ports.clock);
    let answer = writer.take_out(&written, &fresh).await;
    let (outcome, said, why) = match answer {
        _ if written.unsent() => (TakenOut::Unsent, None, None),
        Err(unsent) => (TakenOut::Unsent, None, Some(unsent.into_reason())),
        Ok(Answered::Done) => (TakenOut::Out, None, None),
        Ok(Answered::Refused(said)) => {
            let outcome = back(read, fresh.number, TakenOut::Refused).await;
            (outcome, Some(said), None)
        }
        Ok(Answered::Lost) => (
            back(read, fresh.number, TakenOut::Unsettled).await,
            None,
            None,
        ),
    };
    settled(store, (record, at), (outcome, said, why)).await
}

#[cfg(test)]
mod tests;
