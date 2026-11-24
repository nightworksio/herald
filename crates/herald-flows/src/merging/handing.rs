//! The hand-off: each pull request due handed to the forge's merge queue
//! or merge train with the queue's own write credential. Right before the
//! call, the pull request is read again on its own with nothing kept from
//! the queue's read, and every input the hand-off stands on is checked at
//! the head handed over; the record is written before the call, and what
//! came of it after (ADR-0087).

use std::sync::Arc;

use herald_config::MergeQueue;
use herald_core::merging::{GestureRecord, HandOff, HandedOff, Heads, Left, unvouched};
use herald_kinds::{Branched, Entered, Handed, MergeReader, Source, WriteWith};
use herald_ports::{
    Actions, Base, Clock, DynHttp, Http, Signer, Store, Version, Versioned, Written,
};
use herald_wire::github_app::handing_off;

use super::{Flow, Found, Handing, Judged, Next, Took, UNWRITTEN, asked, gesture_key, version};
use crate::Ports;
use crate::applying::{Writing, held_apart, minted_with};
use crate::budget::Budgeted;

/// Why a pull request is not handed over where reading it again finds it
/// closed, into another branch, or without a gesture standing.
pub const GONE: &str = "read again right before the step, it is no longer open into the branch, or no gesture by one holding the queue's least role stands on it, so nothing was handed over";

/// Why a pull request is not handed over where another gesture stands now.
pub const REGESTURED: &str = "read again right before the step, the gesture standing on it is another one, so nothing was handed over; the next run reads it afresh";

/// Why a pull request is not handed over where the forge named no id for
/// its gesture.
pub const UNNAMED: &str = "the forge named no id for the gesture, so herald can keep no record of it and hands nothing over";

/// Why a pull request is not handed over where reading it again was not
/// whole.
pub const UNREAD_AGAIN: &str = "read again right before the step, what decides whether it stands was not read whole, so nothing was handed over";

/// Why a pull request is not handed over where its gesturer's role is
/// below the queue's least now.
pub const UNROLED: &str = "read again right before the step, the person who queued it no longer holds the queue's least role, so nothing was handed over";

/// Why a pull request is not handed over where it is a draft now.
pub const DRAFTED: &str =
    "read again right before the step, it is a draft, so nothing was handed over";

/// Why a pull request is not handed over where herald holds no record of
/// the head its gesture vouches for.
pub const UNRECORDED_HEAD: &str =
    "herald holds no record of the head its gesture vouches for, so nothing was handed over";

/// Why a pull request is not handed over where another run recorded its
/// hand-off first.
pub const RACED: &str =
    "another run recorded handing it over for this gesture first, so it is not handed over twice";

/// One queue made ready to hand pull requests over: its source, its write
/// credential held apart, the name it is recorded by, and a connection to
/// the forge's API alone.
struct Ready<H> {
    source: Source,
    writing: Writing,
    credential: String,
    written: H,
}

/// `queue`'s write credential read through `handing` and held apart from
/// every credential its source reads with, and a connection made to its
/// forge's API alone.
///
/// # Errors
///
/// Where it names no write credential, its source is gone, a credential
/// cannot be read or is the read one, or no connection can be made.
async fn ready<S: Store, C: Clock, G: Signer, H: Http>(
    flow: Flow<'_, '_, S, C, G, H>,
    handing: &Handing<'_, H>,
    queue: &MergeQueue,
) -> Result<Ready<H>, String> {
    let write = queue.writing().ok_or(UNWRITTEN)?;
    let credential = write.name().to_owned();
    let unconfigured = format!("the source {} is not configured", queue.source.as_str());
    let source = flow.0.sources.get(&queue.source).ok_or(unconfigured)?;
    let held = held_apart(flow, handing.secrets, (&queue.source, source, write));
    let (writing, _) = held.await?;
    let written = (handing.connect)(Base::At(&queue.forge.api))?;
    Ok(Ready {
        source: source.clone(),
        writing,
        credential,
        written,
    })
}

/// Why `fresh`, the pull request read again right before the step, may not
/// be handed over as `entered`, read with the queue, was judged due, with
/// `seen` the head herald first read with its gesture: each input the
/// hand-off stands on, at the head handed over.
fn unauthorised(
    queue: &MergeQueue,
    (entered, fresh): (&Entered, &Entered),
    seen: Option<&str>,
) -> Option<Took> {
    let refused = |why: &str| Some(Took::Refused(why.to_owned()));
    if !fresh.whole {
        return refused(UNREAD_AGAIN);
    }
    if fresh.gesture.id != entered.gesture.id
        || fresh.gesture.by != entered.gesture.by
        || fresh.gesture.at != entered.gesture.at
    {
        return refused(REGESTURED);
    }
    if fresh.gesture.role.is_none_or(|role| role < queue.least) {
        return refused(UNROLED);
    }
    if fresh.draft {
        return refused(DRAFTED);
    }
    if fresh.head != entered.head {
        return Some(Took::HeadMoved);
    }
    let heads = Heads {
        then: fresh.gesture.head.as_deref(),
        named: fresh.gesture.named.as_deref(),
        seen,
        approved: fresh.approved,
    };
    match unvouched(queue.vouches, &heads, &fresh.head)? {
        Left::HeadMoved => Some(Took::HeadMoved),
        _ => refused(UNRECORDED_HEAD),
    }
}

/// What this apply took for `outcome`, the forge having said `said` where
/// it refused, and herald `why` where nothing was sent.
fn took(outcome: HandedOff, said: Option<String>, why: Option<String>) -> Took {
    match outcome {
        HandedOff::Queued => Took::Queued,
        HandedOff::Refused => Took::Declined(said.unwrap_or_default()),
        HandedOff::HeadMoved => Took::HeadMoved,
        HandedOff::Unsettled => Took::Unsettled,
        HandedOff::Calling | HandedOff::Unsent => {
            Took::Refused(why.unwrap_or_else(|| outcome.said().to_owned()))
        }
    }
}

/// `record` written to `store` with its hand-off come to `outcome`, over
/// the version `at`: what this apply took, or that it could not be
/// recorded.
async fn settled<A: Actions>(
    store: &A,
    (mut record, at): (GestureRecord, Version),
    (outcome, said, why): (HandedOff, Option<String>, Option<String>),
) -> Took {
    if let Some(hand_off) = &mut record.hand_off {
        hand_off.outcome = outcome;
    }
    match store.put_gesture(&record, Some(at)).await {
        Ok(Written::Written(_)) => took(outcome, said, why),
        Ok(Written::Conflict) | Err(_) => Took::Unrecorded(outcome),
    }
}

/// What reading `fresh` back after the forge refused it says: its head
/// moved, or the refusal stands.
async fn refused_back(
    (reader, http): (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    fresh: &Entered,
) -> HandedOff {
    match reader
        .entry(http, (&queue.branch, fresh.number), asked(queue))
        .await
    {
        Ok(Some(back)) if back.head != fresh.head => HandedOff::HeadMoved,
        _ => HandedOff::Refused,
    }
}

/// `entered`, judged due in the queue `named`, `queue`, on the base
/// `branched`, handed to the forge's queue as `ready` makes it, its record
/// `kept` where one was read: read again through `reader` and `http` and
/// checked, recorded in `store`, its write App's token minted only then,
/// the call made naming the head the forge must find, and what came of it
/// recorded. A lost answer is read back, never asked again.
async fn handed_off<S: Store, C: Clock, G: Signer, H: Http, A: Actions>(
    (ports, store, ready): (&Ports<'_, S, C, G>, &A, &Ready<H>),
    read: (&dyn MergeReader, &dyn DynHttp),
    (named, queue, branched): (&str, &MergeQueue, &Branched),
    (entered, kept): (&Entered, Option<Versioned<GestureRecord>>),
) -> Took {
    let (reader, http) = read;
    let fresh = match reader
        .entry(http, (&queue.branch, entered.number), asked(queue))
        .await
    {
        Ok(Some(fresh)) => fresh,
        Ok(None) => return Took::Refused(GONE.to_owned()),
        Err(error) => {
            return Took::Refused(format!(
                "it could not be read again right before the step, so nothing was handed over: {}",
                error.into_reason()
            ));
        }
    };
    let seen = kept.as_ref().map(|kept| kept.value.seen.as_str());
    if let Some(refused) = unauthorised(queue, (entered, &fresh), seen) {
        return refused;
    }
    let now = ports.clock.now();
    let Some(key) = gesture_key(queue, &fresh) else {
        return Took::Refused(UNNAMED.to_owned());
    };
    let mut record = kept.as_ref().map_or_else(
        || GestureRecord {
            key,
            queue: named.to_owned(),
            seen: fresh.head.clone(),
            seen_at: now,
            hand_off: None,
        },
        |kept| kept.value.clone(),
    );
    record.hand_off = Some(HandOff {
        head: fresh.head.clone(),
        base: branched.head.clone(),
        credential: ready.credential.clone(),
        at: now,
        outcome: HandedOff::Calling,
    });
    let at = match store.put_gesture(&record, version(kept.as_ref())).await {
        Ok(Written::Written(at)) => at,
        Ok(Written::Conflict) => return Took::Refused(RACED.to_owned()),
        Err(error) => {
            return Took::Refused(format!(
                "herald's record of it could not be written, so nothing was handed over: {error}"
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
            return settled(store, (record, at), (HandedOff::Unsent, None, Some(why))).await;
        }
    };
    let written = Budgeted::new(&ready.written, ports.budget, ports.clock);
    let answer = writer.hand_off(&written, &fresh).await;
    let (outcome, said, why) = match answer {
        _ if written.unsent() => (HandedOff::Unsent, None, None),
        Err(unsent) => (HandedOff::Unsent, None, Some(unsent.into_reason())),
        Ok(Handed::Queued) => (HandedOff::Queued, None, None),
        Ok(Handed::Refused(said)) => (refused_back(read, queue, &fresh).await, Some(said), None),
        Ok(Handed::Lost) => match reader.queued(http, fresh.number).await {
            Ok(true) => (HandedOff::Queued, None, None),
            _ => (HandedOff::Unsettled, None, None),
        },
    };
    settled(store, (record, at), (outcome, said, why)).await
}

/// `judged`, the entries of the queue `named`, `queue`, on the base
/// `branched`, each one due handed over with the write credential `flow`
/// and `handing` make ready, once, and only where one is due, its record
/// kept in `store`.
pub(super) async fn stepped<S: Store, C: Clock, G: Signer, H: Http, A: Actions>(
    (flow, handing): (Flow<'_, '_, S, C, G, H>, &Handing<'_, H>),
    store: &A,
    read: (&dyn MergeReader, &dyn DynHttp),
    (named, queue): (&str, &MergeQueue),
    branched: &Branched,
    judged: Vec<(Judged, Option<Versioned<GestureRecord>>)>,
) -> Found {
    if !judged
        .iter()
        .any(|(judged, _)| judged.next == Next::HandOff)
    {
        return Found::Entries(judged.into_iter().map(|(judged, _)| judged).collect());
    }
    let prepared = ready(flow, handing, queue).await;
    let mut entries = Vec::with_capacity(judged.len());
    for (judged, kept) in judged {
        if judged.next != Next::HandOff {
            entries.push(judged);
            continue;
        }
        let took = match &prepared {
            Ok(ready) => {
                let made = (flow.2, store, ready);
                let gestured = (&judged.entered, kept);
                handed_off(made, read, (named, queue, branched), gestured).await
            }
            Err(why) => Took::Refused(why.clone()),
        };
        entries.push(Judged {
            next: Next::Took(took),
            ..judged
        });
    }
    Found::Entries(entries)
}

#[cfg(test)]
mod tests;
