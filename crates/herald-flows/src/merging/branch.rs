//! The base branch of one merge queue, read from its forge with its
//! source's read credential, as `herald setup merge-queue` shows it before
//! the operator chooses who runs the queue (ADR-0087).

use herald_config::Config;
use herald_core::Printed;
use herald_kinds::Branched;
use herald_ports::{Clock, Http, Signer, Store};

use crate::{Connected, Ports};

/// The base branch of the merge queue `name`, read now, or why it was not,
/// the why only where `printed` prints failures; nothing is written and no
/// write credential is read.
///
/// # Errors
///
/// Where the branch was not read: why, where `printed` prints failures.
pub async fn merge_branch<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, printed): (&str, Printed),
) -> Result<Branched, Option<String>> {
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
    reading
        .reader
        .branch(http, &reading.branch)
        .await
        .map_err(|error| failed(error.into_reason()))
}
