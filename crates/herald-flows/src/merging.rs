//! `herald merge-queue`: each merge queue's entries read from its forge
//! with its source's read credential, each with what the queue does next
//! and why. The dry run writes nothing and reads no write credential; the
//! apply hands each pull request due to the forge's queue, after a fresh
//! read of it and a record written first (ADR-0087).

use herald_config::{Config, MergeQueue};
use herald_core::Printed;
use herald_core::SourceName;
use herald_core::merging::{
    AppliedBy, GestureKey, GestureRecord, HandedOff, Heads, Left, Runs, Step, TakenOut, Verdict,
    Vouches, unvouched,
};
use herald_kinds::{Branched, EnterAsked, Entered, Marked, MergeReader, TakenBack, Unpolled};
use herald_ports::{
    Actions, Base, Boxed, Clock, DynHttp, DynSecrets, DynTimer, Http, Signer, Store, Version,
    Versioned, Written,
};
use jiff::Timestamp;

use crate::{Connected, Ports, Stop};

mod branch;
mod handing;
mod owning;
mod taking;

pub use branch::merge_branch;
pub use handing::{
    DRAFTED, GONE, RACED, REGESTURED, UNNAMED, UNREAD_AGAIN, UNRECORDED_HEAD, UNROLED,
};
pub use owning::{Acted, METHODS_UNREAD, OWN_DRAFTED, OWN_REGESTURED, OWN_UNSAID, UNDUE, allowed};
pub use taking::{
    RACED_OUT, RECANCELLED, UNHANDED, UNMATCHED_OUT, UNNAMED_CANCEL, UNREAD_CANCEL, UNROLED_CANCEL,
    UNSAID_ROLE, UNTAKEN,
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

/// Why `herald server` takes no step in a queue `herald merge-queue
/// --apply` runs.
pub const COMMANDED: &str = "applied_by = \"command\": herald merge-queue --apply takes its steps, and herald server takes none";

/// Why a queue run by GitLab takes no step where GitLab does not say the
/// project has merge trains on.
const TRAINS_UNSAID: &str = "GitLab does not say the project has merge trains on, and its merge train API does not say what adding a merge request does where they are off, so with runs = \"forge\" herald hands nothing over; turn merge trains on, or give runs = \"herald\"";

/// The forge whose merge trains must be said to be on before a hand-off.
const GITLAB: &str = "gitlab";

/// Why a queue run by the forge takes no step where the forge says it has
/// no merge queue on the branch.
/// Why an entry is not stepped where herald's record of its gesture's id
/// names another maker or time than the forge does now.
pub const UNMATCHED: &str = "herald's record of this gesture's id names another maker or time than the forge does now, so it is not the same gesture and herald writes nothing for it";

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
    /// herald's own queue: a step recorded for this gesture holds it, as
    /// the forge may or may not have taken it, or is still taking it.
    Held(Step),
    /// herald's own queue: what this apply did with it.
    Acted(Acted),
}

/// What an apply did with a pull request due to be handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Took {
    /// The forge took it into its queue, or reading it back found it
    /// there.
    Queued,
    /// It was already in the forge's queue right before the step, so no
    /// call was made and it was recorded as found there.
    FoundQueued,
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

/// What a queue does with a pull request in the forge's queue whose
/// gesture was taken back, or whose gesture's maker no longer holds the
/// queue's least role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    /// It is taken out of the forge's queue.
    TakeOut,
    /// It would be taken out, but the queue takes no step, as its read
    /// says why.
    Idle,
    /// A cancel herald could not carry out, or a role lost it could not
    /// take it out for, and why: it stays in the forge's queue.
    Uncarried(String),
    /// The person whose gesture herald handed over no longer holds the
    /// queue's least role, and the queue's `role_lost = "stay"` leaves it
    /// in the forge's queue.
    Stays,
    /// herald recorded taking it out for this cancel at `at`, and it came
    /// to `outcome`; it is not asked again.
    Recorded {
        /// What came of it.
        outcome: TakenOut,
        /// When herald recorded it.
        at: Timestamp,
    },
    /// What this apply did with it.
    Took(TookOut),
}

/// What an apply did with a pull request due to be taken out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TookOut {
    /// The forge took it out of its queue, or reading it back found it
    /// out.
    Out,
    /// The forge refused to take it out, saying this, and it is still
    /// there.
    Declined(String),
    /// The forge's answer was lost and it is still in the forge's queue;
    /// herald never asks again for this cancel.
    Unsettled,
    /// The call came to this, but herald's record of it could not be
    /// updated, so the record still says the call is being made and herald
    /// never asks again for this cancel.
    Unrecorded(TakenOut),
}

/// A pull request in the forge's queue whose gesture was taken back, or
/// whose gesture's maker no longer holds the queue's least role, with what
/// the queue does about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancel {
    /// The pull request and its marks, as the forge says.
    pub taken: TakenBack,
    /// The cancel judged against the gesture herald handed over: the one
    /// that takes it out, or one herald could not carry out; none where it
    /// is taken out, or stays, for the role lost.
    pub cancel: Option<Marked>,
    /// What the queue does.
    pub next: Undo,
    /// Who herald recorded made the gesture it handed over, where the forge
    /// names someone else for it now, as for an account gone.
    pub renamed: Option<String>,
}

/// What a merge queue's read of the cancels found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cancels {
    /// Each pull request in the forge's queue whose gesture was taken
    /// back; none where its entries were not read.
    Read(Vec<Cancel>),
    /// They could not be read, and why where the output may say.
    Unread(Option<String>),
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
    /// The pull requests in the forge's queue whose gesture was taken
    /// back.
    pub cancels: Cancels,
}

/// What a run steps a queue through, where it steps any, and whose steps
/// it takes.
type Stepping<'a, 'h, H> = (Option<&'a Handing<'h, H>>, AppliedBy);

/// A run's config, connections and ports.
type Flow<'f, 'p, S, C, G, H> = (&'f Config, &'f Connected<H>, &'f Ports<'p, S, C, G>);

/// What an apply reaches beside a run's own connections: the host's
/// secrets, for each queue's write credential alone, a connection made
/// for each base, and what waits between reads of a head herald's own
/// update has not moved yet.
pub struct Handing<'a, H> {
    /// The host's secrets.
    pub secrets: &'a dyn DynSecrets,
    /// A connection reaching only `base`.
    pub connect: &'a (dyn Fn(Base<'_>) -> Result<H, String> + Sync),
    /// What waits between reads of a head.
    pub timer: &'a dyn DynTimer,
}

impl<H> std::fmt::Debug for Handing<'_, H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handing").finish_non_exhaustive()
    }
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

/// Why a run of `by` takes no step in `queue`, on the base `branched`,
/// with `store` keeping records where it is some: each reason it does not
/// start, or that its steps are another's to take.
fn idle<A>(
    queue: &MergeQueue,
    (branched, by): (&Branched, AppliedBy),
    store: Option<&A>,
) -> Vec<String> {
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
    if queue.applied_by != by {
        idle.push(
            match by {
                AppliedBy::Command => SERVED,
                AppliedBy::Server => COMMANDED,
            }
            .to_owned(),
        );
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
    })
}

/// The record of `entered`'s gesture in the queue `named` as `keeping`
/// keeps it, with the head herald first read with the gesture standing:
/// where none is kept and `queue` vouches for that head, an apply records
/// the head it reads now, as of `now`, and a dry run takes it as the one a
/// real run would record. Where the record cannot be read or written, no
/// head was first seen.
///
/// # Errors
///
/// [`UNMATCHED`], where the record kept names another maker or time for
/// the gesture than `entered` does.
async fn seen<A: Actions>(
    keeping: &Keeping<'_, A>,
    (named, queue): (&str, &MergeQueue),
    entered: &Entered,
    now: Timestamp,
) -> Result<(Option<Versioned<GestureRecord>>, Option<String>), &'static str> {
    let Some(key) = gesture_key(queue, entered) else {
        return Ok((None, None));
    };
    let store = &keeping.store;
    Ok(match store.gesture(&key).await {
        Ok(Some(kept))
            if kept.value.by != entered.gesture.by || kept.value.at != entered.gesture.at =>
        {
            return Err(UNMATCHED);
        }
        Ok(Some(kept)) => {
            let seen = kept.value.seen.clone();
            (Some(kept), Some(seen))
        }
        Ok(None) if queue.vouches == Vouches::Gesture => (None, None),
        Ok(None) if !keeping.applies => (None, Some(entered.head.clone())),
        Ok(None) => {
            let record = first_record(key, named, entered, now);
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
    })
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
) -> Result<Vec<(Judged, Option<Versioned<GestureRecord>>)>, Unpolled> {
    let mut judged = Vec::with_capacity(entries.len());
    let mut flying = false;
    for entered in entries {
        let read = if entered.whole {
            seen(keeping, (named, queue), &entered, now).await
        } else {
            Ok((None, None))
        };
        let (kept, seen) = match read {
            Ok(read) => read,
            Err(why) => {
                let next = Next::Took(Took::Refused(why.to_owned()));
                judged.push((Judged { entered, next }, None));
                continue;
            }
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
        } else if queue.runs == Runs::Herald {
            let gestured = (&entered, seen.as_deref(), kept.as_ref());
            let at = (now, &mut flying);
            owning::next((reader, http), queue, gestured, branched, at).await?
        } else if let Some(left) = unvouched(queue.vouches, &heads, &entered.head) {
            Next::Verdict(Verdict::Leaves(left))
        } else if entered.draft {
            Next::Draft
        } else if let Some(handed) = handed {
            Next::Handed {
                outcome: handed.outcome,
                at: handed.at,
            }
        } else {
            Next::HandOff
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
    let names = named(flow.0, name)?;
    Ok(read_queues(flow, (None, AppliedBy::Command), (&names, printed)).await)
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
    let names = named(flow.0, name)?;
    let by = (Some(handing), AppliedBy::Command);
    Ok(read_queues(flow, by, (&names, printed)).await)
}

/// Every merge queue whose steps `herald server` takes (`applied_by =
/// "server"`), of the source `source` where it is some, as a webhook of it
/// wakes them, read now and stepped as [`merge_apply`] steps a queue, with
/// the queue's own write credential read through `handing` alone; a
/// private or internal repository's entries are stepped and withheld from
/// what is said (ADR-0087). None where no such queue is configured.
pub async fn merge_served<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    handing: &Handing<'_, H>,
    source: Option<&SourceName>,
) -> Vec<MergeQueueRead> {
    let names = served(flow.0, source);
    if names.is_empty() {
        return Vec::new();
    }
    let by = (Some(handing), AppliedBy::Server);
    read_queues(flow, by, (&names, Printed::Withheld)).await
}

/// The queues whose steps `herald server` takes, of `source` where it is
/// some.
#[must_use]
pub fn served<'c>(config: &'c Config, source: Option<&SourceName>) -> Vec<&'c str> {
    config
        .merge_queues
        .iter()
        .filter(|(_, queue)| {
            queue.applied_by == AppliedBy::Server
                && source.is_none_or(|source| &queue.source == source)
        })
        .map(|(name, _)| name.as_str())
        .collect()
}

/// The queues `names` names read, and stepped through `handing` where it
/// is some, as a run of `by`.
async fn read_queues<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (handing, by): (Option<&Handing<'_, H>>, AppliedBy),
    (names, printed): (&[&str], Printed),
) -> Vec<MergeQueueRead> {
    let (config, connected, _) = flow;
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
            let (found, idle, cancels) = if let Some(stop) = stopped.get(&source.name) {
                let why = printed.prints_failures().then(|| unminted(stop));
                (Found::Unread(why), Vec::new(), Cancels::Read(Vec::new()))
            } else {
                let reader = (reading.reader.as_ref(), http as &dyn DynHttp);
                let queued = (reading.name.as_str(), queue);
                queue_read((flow, (handing, by)), reader, queued, printed).await
            };
            read.push(MergeQueueRead {
                name: reading.name.clone(),
                repository: queue.forge.repository.clone(),
                branch: queue.branch.clone(),
                runs: queue.runs,
                idle,
                found,
                cancels,
            });
        }
    }
    read.sort_by(|one, other| one.name.cmp(&other.name));
    read
}

/// The queue `queued` read through `reader` as `printed` prints it, and
/// stepped through `handing` where it is some and the queue takes steps;
/// with why it takes none, and its cancels.
async fn queue_read<S: Store, C: Clock, G: Signer, H: Http>(
    (flow, (handing, by)): (Flow<'_, '_, S, C, G, H>, Stepping<'_, '_, H>),
    reader: (&dyn MergeReader, &dyn DynHttp),
    queued: (&str, &MergeQueue),
    printed: Printed,
) -> (Found, Vec<String>, Cancels) {
    let ports = flow.2;
    let store = ports.store.actions();
    let asked = (queued, store, (handing.is_some(), by));
    let due = match found(reader, asked, printed, ports.clock.now()).await {
        Reading::Done(found) => return (found, Vec::new(), Cancels::Read(Vec::new())),
        Reading::Due(due) => due,
    };
    let withheld = due.withheld;
    let (found, idle, cancels) = stepped_queue((flow, handing), reader, queued, due).await;
    if let Some(withheld) = withheld {
        return (
            Found::Withheld(withheld),
            Vec::new(),
            Cancels::Read(Vec::new()),
        );
    }
    (found, idle, read_cancels(cancels, printed))
}

/// The queue `queued`, read as `due`, stepped through `handing` where it is
/// some and the queue takes steps; with why it takes none, and its
/// cancels.
async fn stepped_queue<S: Store, C: Clock, G: Signer, H: Http>(
    (flow, handing): (Flow<'_, '_, S, C, G, H>, Option<&Handing<'_, H>>),
    reader: (&dyn MergeReader, &dyn DynHttp),
    queued: (&str, &MergeQueue),
    due: Due,
) -> (Found, Vec<String>, Result<Vec<Cancel>, Unpolled>) {
    let ports = flow.2;
    let store = ports.store.actions();
    let Due {
        judged,
        branched,
        idle,
        cancels,
        ..
    } = due;
    let stepping = handing.zip(store).filter(|_| idle.is_empty());
    let Some((handing, store)) = stepping else {
        let found = judged
            .into_iter()
            .map(|(judged, _)| idled(judged, &idle))
            .collect();
        let cancels = cancels.map(|cancels| {
            cancels
                .into_iter()
                .map(|(cancel, _)| idled_out(cancel, &idle))
                .collect()
        });
        return (Found::Entries(found), idle, cancels);
    };
    if queued.1.runs == Runs::Herald {
        let found = owning::stepped((flow, handing), store, reader, queued, judged).await;
        return (found, idle, Ok(Vec::new()));
    }
    let handing_off = judged
        .iter()
        .any(|(judged, _)| judged.next == Next::HandOff);
    let taking_out = cancels.as_ref().is_ok_and(|cancels| {
        cancels
            .iter()
            .any(|(cancel, _)| cancel.next == Undo::TakeOut)
    });
    if !(handing_off || taking_out) {
        let found = judged.into_iter().map(|(judged, _)| judged).collect();
        let cancels =
            cancels.map(|cancels| cancels.into_iter().map(|(cancel, _)| cancel).collect());
        return (Found::Entries(found), idle, cancels);
    }
    let prepared = handing::ready(flow, handing, queued.1).await;
    let made = (ports, &prepared);
    let found = handing::stepped(made, store, reader, queued, &branched, judged).await;
    let cancels = match cancels {
        Ok(cancels) => Ok(taken_out(made, store, reader, queued.1, cancels).await),
        Err(error) => Err(error),
    };
    (found, idle, cancels)
}

/// Each of `cancels` due taken out of the forge's queue as `prepared`
/// makes it ready, its record kept in `store`.
async fn taken_out<S: Store, C: Clock, G: Signer, H: Http, A: Actions>(
    (ports, prepared): (&Ports<'_, S, C, G>, &Result<handing::Ready<H>, String>),
    store: &A,
    read: (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    cancels: Vec<(Cancel, Kept)>,
) -> Vec<Cancel> {
    let mut done = Vec::with_capacity(cancels.len());
    for (cancel, kept) in cancels {
        let next = match (&cancel.next, kept, prepared) {
            (Undo::TakeOut, Some(kept), Ok(ready)) => {
                let made = (ports, store, ready);
                taking::taken_out(made, read, queue, (&cancel, kept)).await
            }
            (Undo::TakeOut, _, Err(why)) => Undo::Uncarried(why.clone()),
            (next, ..) => next.clone(),
        };
        done.push(Cancel { next, ..cancel });
    }
    done
}

/// `cancels` as the output says them: their read's error said only where
/// `printed` prints failures.
fn read_cancels(cancels: Result<Vec<Cancel>, Unpolled>, printed: Printed) -> Cancels {
    match cancels {
        Ok(cancels) => Cancels::Read(cancels),
        Err(error) => Cancels::Unread(printed.prints_failures().then(|| error.into_reason())),
    }
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

/// `cancel`, its take-out held back where the queue takes no step.
fn idled_out(cancel: Cancel, idle: &[String]) -> Cancel {
    match cancel.next {
        Undo::TakeOut if !idle.is_empty() => Cancel {
            next: Undo::Idle,
            ..cancel
        },
        _ => cancel,
    }
}

/// The record of a gesture, where one is kept.
type Kept = Option<Versioned<GestureRecord>>;

/// A queue's entries read and judged, each with its gesture's record where
/// one is kept, with the base, why the queue takes no step, and how many
/// entries are withheld from what is said where they are.
struct Due {
    judged: Vec<(Judged, Kept)>,
    branched: Branched,
    idle: Vec<String>,
    cancels: Result<Vec<(Cancel, Kept)>, Unpolled>,
    withheld: Option<usize>,
}

/// What a queue's read came to: done, or entries a step may be due on.
enum Reading {
    Done(Found),
    Due(Due),
}

/// What the queue `named`, `queue`, read through `reader` finds as of
/// `now`, as `printed` prints it, each gesture's record read from `store`
/// and, where `applies` and the queue takes steps in a run of `by`,
/// written.
async fn found<A: Actions>(
    (reader, http): (&dyn MergeReader, &dyn DynHttp),
    ((named, queue), store, (applies, by)): ((&str, &MergeQueue), Option<&A>, (bool, AppliedBy)),
    printed: Printed,
    now: Timestamp,
) -> Reading {
    let unread = |error: Unpolled| {
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
    let withheld = (!printed.prints(branched.visibility)).then_some(entries.len());
    let idle = idle(queue, (&branched, by), store);
    let keeping = Keeping {
        store,
        applies: applies && idle.is_empty(),
    };
    let gestures = (named, queue, &keeping);
    let judged = match judged((reader, http), gestures, (entries, &branched), now).await {
        Ok(judged) => judged,
        Err(error) => return unread(error),
    };
    let taken = if queue.runs == Runs::Forge {
        reader.taken_back(http, &queue.branch, asked(queue)).await
    } else {
        Ok(Vec::new())
    };
    let cancels = match taken {
        Ok(taken) => {
            let mut cancels = Vec::with_capacity(taken.len());
            for taken in taken {
                cancels.extend(taking::undone(store, queue, taken).await);
            }
            Ok(cancels)
        }
        Err(error) => Err(error),
    };
    Reading::Due(Due {
        judged,
        branched,
        idle,
        cancels,
        withheld,
    })
}

/// The first record of `entered`'s gesture, keyed `key`, in the queue
/// `named`, its head first seen `now`.
fn first_record(key: GestureKey, named: &str, entered: &Entered, now: Timestamp) -> GestureRecord {
    GestureRecord {
        key,
        by: entered.gesture.by.clone(),
        at: entered.gesture.at,
        queue: named.to_owned(),
        seen: entered.head.clone(),
        seen_at: now,
        hand_off: None,
        taken_out: Vec::new(),
        own: herald_core::merging::Own::default(),
    }
}

/// What `queue` asks of the gestures that queue its pull requests.
const fn asked(queue: &MergeQueue) -> EnterAsked<'_> {
    EnterAsked {
        enter: &queue.enter,
        least: queue.least,
        vouches: queue.vouches,
        skew: queue.skew,
        updated: queue.updated,
    }
}

/// The pull request `number` read again through `read` into `queue`'s
/// branch, as `queue` asks of the gestures that queue it.
fn again<'a>(
    (reader, http): (&'a dyn MergeReader, &'a dyn DynHttp),
    queue: &'a MergeQueue,
    number: u64,
) -> Boxed<'a, Result<Option<Entered>, Unpolled>> {
    reader.entry(http, (&queue.branch, number), asked(queue))
}

/// The version `kept` is at, where a record is kept.
fn version(kept: Option<&Versioned<GestureRecord>>) -> Option<Version> {
    kept.map(|kept| kept.version)
}

#[cfg(test)]
mod tests;
