//! `herald merge-queue`'s dry run: each merge queue's entries read from its
//! forge with its source's read credential, each with what the queue does
//! next and why; nothing is written and no write credential is read
//! (ADR-0087).

use herald_config::{Config, MergeQueue};
use herald_core::Printed;
use herald_core::merging::{Heads, Left, Read, Runs, Verdict, unvouched, verdict};
use herald_kinds::{Branched, EnterAsked, Entered, MergeReader};
use herald_ports::{Clock, DynHttp, Http, Signer, Store};
use jiff::Timestamp;

use crate::{Connected, Ports, Stop};

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

/// One merge queue's dry run.
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
    /// What its read found.
    pub found: Found,
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

/// What `queue`'s entries `entries`, on the base `branched`, come to as of
/// `now`, each one proved on its base read through `reader` and `http` as
/// it comes in flight; a dry run keeps no record of the head it first saw,
/// so a gesture that vouches for that head vouches for none.
async fn judged(
    (reader, http): (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    (entries, branched): (Vec<Entered>, &Branched),
    now: Timestamp,
) -> Result<Vec<Judged>, herald_kinds::Unpolled> {
    let mut judged = Vec::with_capacity(entries.len());
    let mut flying = false;
    for entered in entries {
        let heads = Heads {
            then: entered.gesture.head.as_deref(),
            named: entered.gesture.named.as_deref(),
            seen: None,
            approved: entered.approved,
        };
        let next = if !entered.whole {
            Next::Verdict(Verdict::Leaves(Left::Unread))
        } else if let Some(left) = unvouched(queue.vouches, &heads, &entered.head) {
            Next::Verdict(Verdict::Leaves(left))
        } else if queue.runs == Runs::Forge {
            if entered.draft {
                Next::Draft
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
        judged.push(Judged { entered, next });
    }
    Ok(judged)
}

/// The merge queue `name`, or every one, read now from its forge with its
/// source's read credential, a private or internal repository's entries
/// only as `printed` prints them: nothing is written, and no write
/// credential is read (ADR-0087).
///
/// # Errors
///
/// Where no merge queue has the name.
pub async fn merge_queues<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, printed): (Option<&str>, Printed),
) -> Result<Vec<MergeQueueRead>, Stop> {
    let (config, connected, ports) = flow;
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
            let found = match stopped.get(&source.name) {
                Some(stop) => Found::Unread(printed.prints_failures().then(|| unminted(stop))),
                None => {
                    found(
                        (reading.reader.as_ref(), http),
                        queue,
                        printed,
                        ports.clock.now(),
                    )
                    .await
                }
            };
            read.push(MergeQueueRead {
                name: reading.name.clone(),
                repository: queue.forge.repository.clone(),
                branch: queue.branch.clone(),
                runs: queue.runs,
                found,
            });
        }
    }
    read.sort_by(|one, other| one.name.cmp(&other.name));
    Ok(read)
}

/// What `queue`'s read through `reading` finds as of `now`, as `printed`
/// prints it.
async fn found(
    (reader, http): (&dyn MergeReader, &dyn DynHttp),
    queue: &MergeQueue,
    printed: Printed,
    now: Timestamp,
) -> Found {
    let unread = |error: herald_kinds::Unpolled| {
        Found::Unread(printed.prints_failures().then(|| error.into_reason()))
    };
    let branched = match reader.branch(http, &queue.branch).await {
        Ok(branched) => branched,
        Err(error) => return unread(error),
    };
    let entries = match reader
        .entered(
            http,
            &queue.branch,
            EnterAsked {
                enter: &queue.enter,
                least: queue.least,
                vouches: queue.vouches,
                skew: queue.skew,
            },
        )
        .await
    {
        Ok(entries) => entries,
        Err(error) => return unread(error),
    };
    if !printed.prints(branched.visibility) {
        return Found::Withheld(entries.len());
    }
    match judged((reader, http), queue, (entries, &branched), now).await {
        Ok(judged) => Found::Entries(judged),
        Err(error) => unread(error),
    }
}

#[cfg(test)]
mod tests;
