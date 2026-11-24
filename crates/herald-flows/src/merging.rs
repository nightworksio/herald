//! `herald merge-queue`: each merge queue's entries read from its forge
//! with its source's read credential, each with what the queue does next
//! and why. The dry run writes nothing and reads no write credential; the
//! apply hands each pull request due to the forge's queue, after a fresh
//! read of it and a record written first (ADR-0087).

use herald_config::{Config, MergeQueue};
use herald_core::Printed;
use herald_core::merging::{
    AppliedBy, GestureKey, GestureRecord, HandedOff, Heads, Left, Read, Runs, Verdict, Vouches,
    unvouched, verdict,
};
use herald_kinds::{Branched, EnterAsked, Entered, MergeReader};
use herald_ports::{
    Actions, Base, Clock, DynHttp, DynSecrets, Http, Signer, Store, Version, Versioned, Written,
};
use jiff::Timestamp;

use crate::{Connected, Ports, Stop};

mod handing;

pub use handing::{
    DRAFTED, GONE, RACED, REGESTURED, UNNAMED, UNREAD_AGAIN, UNRECORDED_HEAD, UNROLED,
};

/// Why a queue without a method does not start.
pub(crate) const METHODLESS: &str = "a merge queue without a method does not start; give method = \"merge\", \"squash\", \"rebase\", \"fast-forward\" or \"repository\"";

/// Why a queue without a write credential does not start.
pub(crate) const UNWRITTEN: &str = "a merge queue without a write credential does not start; give write = { token = { secret = \"NAME\" } }, or on GitHub write = { app = \"<github_apps name>\" }";

/// Why a queue on a store that keeps no records does not start.
pub(crate) const UNRECORDED: &str = "the markers store keeps no record to write before each step, so a merge queue on it does not start; choose a store that keeps records";

/// Why `herald merge-queue --apply` takes no step in a queue the server
/// runs.
pub const SERVED: &str = "applied_by = \"server\": herald server takes its steps, and herald merge-queue --apply takes none";

/// Why `herald merge-queue --apply` takes no step in a queue herald runs.
pub const OWN_QUEUE: &str = "runs = \"herald\": herald merge-queue --apply hands pull requests to the forge's queue, and takes no step in herald's own";

/// Why a queue run by GitLab takes no step where GitLab does not say the
/// project has merge trains on.
const TRAINS_UNSAID: &str = "GitLab does not say the project has merge trains on, and its merge train API does not say what adding a merge request does where they are off, so with runs = \"forge\" herald hands nothing over; turn merge trains on, or give runs = \"herald\"";

/// The forge whose merge trains must be said to be on before a hand-off.
const GITLAB: &str = "gitlab";

/// Why a queue run by the forge takes no step where the forge says it has
/// no merge queue on the branch.
const NO_FORGE_QUEUE: &str = "the forge says the branch has no merge queue or merge train, so with runs = \"forge\" the queue does not start; give runs = \"herald\" to run herald's own";

/// What a queue does next with one of its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// herald's own queue: what it does with the pull request in flight,
    /// or one that leaves wherever it stands.
    Verdict(Verdict),
    /// herald's own queue: it waits its turn behind the one in flight.
    Behind,
    /// The forge's queue: it is handed to the forge's merge queue or
    /// merge train.
    HandOff,
    /// The forge's queue: it waits until it is no draft.
    Draft,
    /// It would be handed over, but the queue takes no step, as its read
    /// says why.
    Idle,
    /// The forge's queue: herald recorded handing it over for this gesture
    /// at `at`, and it came to `outcome`; it is not handed over again.
    Handed {
        /// What came of it.
        outcome: HandedOff,
        /// When herald recorded it.
        at: Timestamp,
    },
    /// The forge's queue: what this apply did with it.
    Took(Took),
}

/// What an apply did with a pull request due to be handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Took {
    /// The forge took it into its queue, or reading it back found it
    /// there.
    Queued,
    /// The forge refused it, saying this.
    Declined(String),
    /// Its head is no longer the one its gesture vouches for: read again
    /// right before the step, or read back after the forge refused it.
    HeadMoved,
    /// The forge's answer was lost and it was not found in the forge's
    /// queue; herald never hands it over again for this gesture.
    Unsettled,
    /// It was not handed over, for this reason.
    Refused(String),
    /// The call came to this, but herald's record of it could not be
    /// updated, so the record still says the call is being made and herald
    /// never hands it over again for this gesture.
    Unrecorded(HandedOff),
}

/// One entry of a queue, with what the queue does next with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged {
    /// The entry, as the forge says.
    pub entered: Entered,
    /// What comes next.
    pub next: Next,
}

/// What a merge queue's read found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// Its entries, oldest gesture first.
    Entries(Vec<Judged>),
    /// Its repository is private or internal and the output is not said
    /// to be, so its entries are withheld: how many.
    Withheld(usize),
    /// It could not be read, and why where the output may say.
    Unread(Option<String>),
}

/// One merge queue's read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeQueueRead {
    /// The queue's name, as `[merge_queues.<name>]` names it.
    pub name: String,
    /// Its repository's full path.
    pub repository: String,
    /// The base branch it merges into.
    pub branch: String,
    /// Who runs it.
    pub runs: Runs,
    /// Why `herald merge-queue --apply` takes no step in it, where it
    /// takes none, said beside its entries alone.
    pub idle: Vec<String>,
    /// What its read found.
    pub found: Found,
}

/// A run's config, connections and ports.
type Flow<'f, 'p, S, C, G, H> = (&'f Config, &'f Connected<H>, &'f Ports<'p, S, C, G>);

/// What an apply reaches beside a run's own connections: the host's
/// secrets, for each queue's write credential alone, and a connection made
/// for each base.
pub struct Handing<'a, H> {
    /// The host's secrets.
    pub secrets: &'a dyn DynSecrets,
    /// A connection reaching only `base`.
    pub connect: &'a (dyn Fn(Base<'_>) -> Result<H, String> + Sync),
}

/// The records a queue's read keeps of each gesture, and whether it writes
/// them: a dry run reads them alone.
struct Keeping<'s, A> {
    store: Option<&'s A>,
    applies: bool,
}

/// Why `queue` does not start, each reason: no method, no write
/// credential, a store that keeps no records (`markers`), or a forge with
/// no merge queue where the forge runs it.
#[must_use]
pub(crate) fn unstarted(queue: &MergeQueue, markers: bool) -> Vec<(&'static str, String)> {
    let forge_less = queue
        .forge
        .queueless
        .filter(|_| queue.runs == Runs::Forge)
        .map(|why| {
            format!(
                "{why}, so with runs = \"forge\" the queue does not start; give runs = \"herald\" to run herald's own"
            )
        });
    [
        queue
            .method
            .is_none()
            .then(|| ("method", METHODLESS.to_owned())),
        queue
            .write
            .is_none()
            .then(|| ("write", UNWRITTEN.to_owned())),
        markers.then(|| ("store", UNRECORDED.to_owned())),
        forge_less.map(|why| ("runs", why)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Why an apply takes no step in `queue`, on the base `branched`, with
/// `store` keeping records where it is some: each reason it does not
/// start, or that the server, or herald's own queue, takes its steps.
fn idle<A>(queue: &MergeQueue, branched: &Branched, store: Option<&A>) -> Vec<String> {
    let mut idle: Vec<String> = unstarted(queue, store.is_none())
        .into_iter()
        .map(|(_, why)| why)
        .collect();
    if queue.runs == Runs::Forge && branched.queue == Some(false) {
        idle.push(NO_FORGE_QUEUE.to_owned());
    }
    if queue.runs == Runs::Forge && queue.forge.forge == GITLAB && branched.queue.is_none() {
        idle.push(TRAINS_UNSAID.to_owned());
    }
    if queue.runs == Runs::Herald {
        idle.push(OWN_QUEUE.to_owned());
    }
    if queue.applied_by == AppliedBy::Server {
        idle.push(SERVED.to_owned());
    }
    idle
}

/// Why a queue whose source's installation token `stop` kept from being
/// minted is unread.
fn unminted(stop: &Stop) -> String {
    match stop {
        Stop::Unreachable(reason) | Stop::Source(reason) => {
            format!("its source's installation token could not be minted: {reason}")
        }
        _ => "its source's installation token could not be minted".to_owned(),
    }
}

/// The queues `name` names: that one, or every one.
fn named<'c>(config: &'c Config, name: Option<&str>) -> Result<Vec<&'c str>, Stop> {
    match name {
        Some(name) => config
            .merge_queues
            .get_key_value(name)
            .map(|(name, _)| vec![name.as_str()])
            .ok_or_else(|| {
                Stop::Source(format!(
                    "no merge queue is named \"{name}\"; the merge queues are {}",
                    config
                        .merge_queues
                        .keys()
                        .map(|name| format!("\"{name}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }),
        None => Ok(config.merge_queues.keys().map(String::as_str).collect()),
    }
}

/// The key of the record of `entered`'s gesture in `queue`; none where
/// the forge named no id for the gesture, which then takes no step.
fn gesture_key(queue: &MergeQueue, entered: &Entered) -> Option<GestureKey> {
    Some(GestureKey {
        api: queue.forge.api.clone(),
        repository: queue.forge.repository.clone(),
        number: entered.number,
        gesture: entered.gesture.id?,
        by: entered.gesture.by.clone(),
        at: entered.gesture.at,
    })
}

/// The record of `entered`'s gesture in the queue `named` as `keeping`
/// keeps it, with the head herald first read with the gesture standing:
/// where none is kept and `queue` vouches for that head, an apply records
/// the head it reads now, as of `now`, and a dry run takes it as the one a
/// real run would record. Where the record cannot be read or written, no
/// head was first seen.
async fn seen<A: Actions>(
    keeping: &Keeping<'_, A>,
    (named, queue): (&str, &MergeQueue),
    entered: &Entered,
    now: Timestamp,
) -> (Option<Versioned<GestureRecord>>, Option<String>) {
    let Some(key) = gesture_key(queue, entered) else {
        return (None, None);
    };
    let store = &keeping.store;
    match store.gesture(&key).await {
        Ok(Some(kept)) => {
            let seen = kept.value.seen.clone();
            (Some(kept), Some(seen))
        }
        Ok(None) if queue.vouches == Vouches::Gesture => (None, None),
        Ok(None) if !keeping.applies => (None, Some(entered.head.clone())),
        Ok(None) => {
            let record = GestureRecord {
                key,
                queue: named.to_owned(),
                seen: entered.head.clone(),
                seen_at: now,
                hand_off: None,
            };
            match store.put_gesture(&record, None).await {
                Ok(Written::Written(version)) => {
                    let seen = record.seen.clone();
                    let kept = Versioned {
                        value: record,
                        version,
                    };
                    (Some(kept), Some(seen))
                }
                _ => (None, None),
            }
        }
        Err(_) => (None, None),
    }
}

/// What `queue`, named `named`, does next with its entries `entries`, on
/// the base `branched`, as of `now`, each one proved on its base read
/// through `reader` and `http` as it comes in flight, each gesture's record
/// read, and written where it applies, as `keeping` says; with each
/// entry's record where one is kept.
async fn judged<A: Actions>(
    (reader, http): (&dyn MergeReader, &dyn DynHttp),
    (named, queue, keeping): (&str, &MergeQueue, &Keeping<'_, A>),
    (entries, branched): (Vec<Entered>, &Branched),
    now: Timestamp,
) -> Result<Vec<(Judged, Option<Versioned<GestureRecord>>)>, herald_kinds::Unpolled> {
    let mut judged = Vec::with_capacity(entries.len());
    let mut flying = false;
    for entered in entries {
        let (kept, seen) = if entered.whole {
            seen(keeping, (named, queue), &entered, now).await
        } else {
            (None, None)
        };
        let heads = Heads {
            then: entered.gesture.head.as_deref(),
            named: entered.gesture.named.as_deref(),
            seen: seen.as_deref(),
            approved: entered.approved,
        };
        let handed = kept
            .as_ref()
            .filter(|kept| !kept.value.hands_off())
            .and_then(|kept| kept.value.hand_off.as_ref());
        let next = if !entered.whole {
            Next::Verdict(Verdict::Leaves(Left::Unread))
        } else if let Some(left) = unvouched(queue.vouches, &heads, &entered.head) {
            Next::Verdict(Verdict::Leaves(left))
        } else if queue.runs == Runs::Forge {
            if entered.draft {
                Next::Draft
            } else if let Some(handed) = handed {
                Next::Handed {
                    outcome: handed.outcome,
                    at: handed.at,
                }
            } else {
                Next::HandOff
            }
        } else if flying {
            Next::Behind
        } else {
            let proved = reader.proved(http, &entered, branched).await?;
            let since = entered.arrived.map_or(entered.gesture.at, |arrived| {
                arrived.max(entered.gesture.at)
            });
            let read = Read {
                entered: true,
                vouches: queue.vouches,
                heads,
                head: &entered.head,
                up_to_date: proved.up_to_date,
                checks: &proved.checks,
                required: &proved.required,
                mergeable: proved.mergeable,
                waited: now.duration_since(since),
                whole: proved.whole,
            };
            let verdict = verdict(&read, &queue.may_fail, queue.wait);
            flying = !matches!(verdict, Verdict::Leaves(_));
            Next::Verdict(verdict)
        };
        judged.push((Judged { entered, next }, kept));
    }
    Ok(judged)
}

/// The merge queue `name`, or every one, read now from its forge with its
/// source's read credential, a private or internal repository's entries
/// only as `printed` prints them, each gesture's record read from the
/// store: nothing is written, and no write credential is read (ADR-0087).
///
/// # Errors
///
/// Where no merge queue has the name.
pub async fn merge_queues<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, printed): (Option<&str>, Printed),
) -> Result<Vec<MergeQueueRead>, Stop> {
    read_queues(flow, None, (name, printed)).await
}

/// The merge queue `name`, or every one, read now as
/// [`merge_queues`] reads it, and each pull request due handed to the
/// forge's queue with the queue's own write credential, read through
/// `handing` alone, right after a fresh read of it and with its record
/// written first (ADR-0087).
///
/// # Errors
///
/// Where no merge queue has the name.
pub async fn merge_apply<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    handing: &Handing<'_, H>,
    (name, printed): (Option<&str>, Printed),
) -> Result<Vec<MergeQueueRead>, Stop> {
    read_queues(flow, Some(handing), (name, printed)).await
}

/// The queues `name` names read, and stepped through `handing` where it is
/// some.
async fn read_queues<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    handing: Option<&Handing<'_, H>>,
    (name, printed): (Option<&str>, Printed),
) -> Result<Vec<MergeQueueRead>, Stop> {
    let (config, connected, _) = flow;
    let names = named(config, name)?;
    let (stopped, _) = crate::poll::minting(flow, |_| false).await;
    let mut read = Vec::with_capacity(names.len());
    for (source, http) in &connected.sources {
        for reading in &source.merging {
            let Some(queue) = names
                .contains(&reading.name.as_str())
                .then(|| config.merge_queues.get(&reading.name))
                .flatten()
            else {
                continue;
            };
            let (found, idle) = if let Some(stop) = stopped.get(&source.name) {
                let why = printed.prints_failures().then(|| unminted(stop));
                (Found::Unread(why), Vec::new())
            } else {
                let reader = (reading.reader.as_ref(), http as &dyn DynHttp);
                let queued = (reading.name.as_str(), queue);
                queue_read((flow, handing), reader, queued, printed).await
            };
            read.push(MergeQueueRead {
                name: reading.name.clone(),
                repository: queue.forge.repository.clone(),
                branch: queue.branch.clone(),
                runs: queue.runs,
                idle,
                found,
            });
        }
    }
    read.sort_by(|one, other| one.name.cmp(&other.name));
    Ok(read)
}

/// The queue `queued` read through `reader` as `printed` prints it, and
/// stepped through `handing` where it is some and the queue takes steps;
/// with why it takes none.
async fn queue_read<S: Store, C: Clock, G: Signer, H: Http>(
    (flow, handing): (Flow<'_, '_, S, C, G, H>, Option<&Handing<'_, H>>),
    reader: (&dyn MergeReader, &dyn DynHttp),
    queued: (&str, &MergeQueue),
    printed: Printed,
) -> (Found, Vec<String>) {
    let ports = flow.2;
    let store = ports.store.actions();
    let asked = (queued, store, handing.is_some());
    let due = match found(reader, asked, printed, ports.clock.now()).await {
        Reading::Done(found) => return (found, Vec::new()),
        Reading::Due(due) => due,
    };
    let stepping = handing.zip(store).filter(|_| due.idle.is_empty());
    let found = if let Some((handing, store)) = stepping {
        let base = &due.branched;
        handing::stepped((flow, handing), store, reader, queued, base, due.judged).await
    } else {
        Found::Entries(
            due.judged
                .into_iter()
                .map(|(judged, _)| idled(judged, &due.idle))
                .collect(),
        )
    };
    (found, due.idle)
}

/// `judged`, its hand-off held back where the queue takes no step.
fn idled(judged: Judged, idle: &[String]) -> Judged {
    match judged.next {
        Next::HandOff if !idle.is_empty() => Judged {
            next: Next::Idle,
            ..judged
        },
        _ => judged,
    }
}

/// A queue's entries read and judged, each with its gesture's record where
/// one is kept, with the base and why the queue takes no step.
struct Due {
    judged: Vec<(Judged, Option<Versioned<GestureRecord>>)>,
    branched: Branched,
    idle: Vec<String>,
}

/// What a queue's read came to: done, or entries a step may be due on.
enum Reading {
    Done(Found),
    Due(Due),
}

/// What the queue `named`, `queue`, read through `reader` finds as of
/// `now`, as `printed` prints it, each gesture's record read from `store`
/// and, where `applies` and the queue takes steps, written.
async fn found<A: Actions>(
    (reader, http): (&dyn MergeReader, &dyn DynHttp),
    ((named, queue), store, applies): ((&str, &MergeQueue), Option<&A>, bool),
    printed: Printed,
    now: Timestamp,
) -> Reading {
    let unread = |error: herald_kinds::Unpolled| {
        Reading::Done(Found::Unread(
            printed.prints_failures().then(|| error.into_reason()),
        ))
    };
    let branched = match reader.branch(http, &queue.branch).await {
        Ok(branched) => branched,
        Err(error) => return unread(error),
    };
    let entries = match reader.entered(http, &queue.branch, asked(queue)).await {
        Ok(entries) => entries,
        Err(error) => return unread(error),
    };
    if !printed.prints(branched.visibility) {
        return Reading::Done(Found::Withheld(entries.len()));
    }
    let idle = idle(queue, &branched, store);
    let keeping = Keeping {
        store,
        applies: applies && idle.is_empty(),
    };
    let gestures = (named, queue, &keeping);
    match judged((reader, http), gestures, (entries, &branched), now).await {
        Ok(judged) => Reading::Due(Due {
            judged,
            branched,
            idle,
        }),
        Err(error) => unread(error),
    }
}

/// What `queue` asks of the gestures that queue its pull requests.
const fn asked(queue: &MergeQueue) -> EnterAsked<'_> {
    EnterAsked {
        enter: &queue.enter,
        least: queue.least,
        vouches: queue.vouches,
        skew: queue.skew,
    }
}

/// The version `kept` is at, where a record is kept.
fn version(kept: Option<&Versioned<GestureRecord>>) -> Option<Version> {
    kept.map(|kept| kept.version)
}

#[cfg(test)]
mod tests;
