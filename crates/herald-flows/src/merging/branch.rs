//! The base branch of one merge queue, read from its forge with its
//! source's read credential, as `herald setup merge-queue` shows it before
//! the operator chooses who runs the queue, and what the forge says of its
//! own queue there that keeps herald's hand-off from starting (ADR-0087).

use herald_config::Config;
use herald_core::Printed;
use herald_core::merging::{Method, Runs};
use herald_kinds::{Branched, Methods};
use herald_ports::{Clock, Http, Signer, Store};

use crate::{Connected, Ports};

/// Why a queue run by GitLab takes no step where GitLab does not say the
/// project has merge trains on.
pub(crate) const TRAINS_UNSAID: &str = "GitLab does not say the project has merge trains on, and its merge train API does not say what adding a merge request does where they are off, so with runs = \"forge\" herald hands nothing over; turn merge trains on, or give runs = \"herald\"";

/// The forge whose merge trains must be said to be on before a hand-off.
const GITLAB: &str = "gitlab";

/// Why a queue run by the forge takes no step where the forge says it has
/// no merge queue on the branch and names no reason.
const NO_FORGE_QUEUE: &str = "the forge says the branch has no merge queue or merge train";

/// What follows why the forge runs no queue herald can hand a pull request
/// to, where the queue says the forge runs it.
pub(crate) const UNSTARTED: &str = ", so with runs = \"forge\" the queue does not start; give runs = \"herald\" to run herald's own";

/// What the forge says of a merge queue's base branch and repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSaid {
    /// The base branch.
    pub branched: Branched,
    /// The methods the repository allows a merge by, where they were read.
    pub methods: Option<Methods>,
}

/// The base branch of the merge queue `name`, read now, with the methods
/// its repository allows, or why the branch was not read, the why only
/// where `printed` prints failures; nothing is written and no write
/// credential is read.
///
/// # Errors
///
/// Where the branch was not read: why, where `printed` prints failures.
pub async fn merge_branch<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, printed): (&str, Printed),
) -> Result<BranchSaid, Option<String>> {
    let (_, connected, _) = flow;
    let found = connected.sources.iter().find_map(|(source, http)| {
        source
            .merging
            .iter()
            .find(|reading| reading.name == name)
            .map(|reading| (source, http, reading))
    });
    let failed = |why: String| printed.prints_failures().then_some(why);
    let Some((source, http, reading)) = found else {
        return Err(failed(format!(
            "no source reads a merge queue named \"{name}\""
        )));
    };
    let (stopped, _) = crate::poll::minting(flow, |_| false).await;
    if let Some(stop) = stopped.get(&source.name) {
        return Err(failed(super::unminted(stop)));
    }
    let branched = reading
        .reader
        .branch(http, &reading.branch)
        .await
        .map_err(|error| failed(error.into_reason()))?;
    let methods = reading.reader.methods(http).await.ok();
    Ok(BranchSaid { branched, methods })
}

/// The method `method` names, `"repository"` taken as the one `methods`
/// allows where they were read whole and allow one alone; none where it
/// names none, or that one cannot be told.
pub(crate) fn resolved(method: Option<Method>, methods: Option<&Methods>) -> Option<Method> {
    match method? {
        Method::Repository => super::allowed(method, methods?).ok(),
        named => Some(named),
    }
}

/// Why a queue run as `runs` says, merging by `method`, on the forge
/// `forge` as a source's kind names it, hands nothing to the forge's queue
/// on `branched`, each with the key at fault and the fix: the forge has
/// none there, GitLab does not say it has, its API takes no hand-off, or
/// its queue merges by another method, `"repository"` taken as `methods`
/// allow.
pub(crate) fn unhanded(
    (runs, method, forge): (Runs, Option<Method>, &str),
    branched: &Branched,
    methods: Option<&Methods>,
) -> Vec<(&'static str, String)> {
    if runs != Runs::Forge {
        return Vec::new();
    }
    let unqueued = match (branched.queue, branched.queued.unhanded.as_deref()) {
        (Some(true) | None, None) => None,
        (Some(false), None) => Some(NO_FORGE_QUEUE),
        (_, Some(why)) => Some(why),
    };
    let unmatched = branched
        .queued
        .method
        .zip(resolved(method, methods))
        .filter(|(own, named)| own != named)
        .map(|(own, named)| {
            format!(
                "the forge's merge queue on the branch merges by \"{}\", and with runs = \"forge\" the method is the forge queue's own, so a queue naming \"{}\" does not start; give method = \"{}\"",
                own.name(),
                named.name(),
                own.name()
            )
        });
    let unsaid = (forge == GITLAB && branched.queue.is_none()).then(|| TRAINS_UNSAID.to_owned());
    unqueued
        .map(|why| format!("{why}{UNSTARTED}"))
        .into_iter()
        .chain(unsaid)
        .map(|why| ("runs", why))
        .chain(unmatched.map(|why| ("method", why)))
        .collect()
}
