//! What doctor says of each merge queue (ADR-0087): where it merges and
//! through whose queue, how, how a branch is brought up to date, its write
//! credential, where its steps are taken, and why one does not start.

use herald_config::{Config, MergeQueue};
use herald_core::Duration;
use herald_core::merging::{AppliedBy, Runs, Vouches};

use super::{Findings, gap};

/// Why a queue without a method does not start.
const METHODLESS: &str = "a merge queue without a method does not start; give method = \"merge\", \"squash\", \"rebase\", \"fast-forward\" or \"repository\"";

/// Why a queue without a write credential does not start.
const UNWRITTEN: &str = "a merge queue without a write credential does not start; give write = { token = { secret = \"NAME\" } }, or on GitHub write = { app = \"<github_apps name>\" }";

/// Why a queue on the markers store does not start.
const UNRECORDED: &str = "the markers store keeps no record to write before each step, so a merge queue on it does not start; choose a store that keeps records";

/// Why `queue`, run by the forge, does not start where its forge has no
/// merge queue.
fn forge_less(queue: &MergeQueue) -> Option<String> {
    let why = queue
        .forge
        .queueless
        .filter(|_| queue.runs == Runs::Forge)?;
    Some(format!(
        "{why}, so with runs = \"forge\" the queue does not start; give runs = \"herald\" to run herald's own"
    ))
}

/// What `queue`, named `name`, does, as doctor says it.
fn said(name: &str, queue: &MergeQueue) -> String {
    let runs = match queue.runs {
        Runs::Forge => "through the forge's merge queue",
        Runs::Herald => "through herald's own queue",
    };
    let method = queue.method.map_or_else(
        || "no method".to_owned(),
        |method| format!("method {}", method.name()),
    );
    let write = queue.writing().map_or_else(
        || "no write credential".to_owned(),
        |write| format!("the write credential {}, held apart", write.name()),
    );
    let applied = match queue.applied_by {
        AppliedBy::Command => "its steps taken by `herald merge-queue --apply`",
        AppliedBy::Server => {
            "its steps taken by `herald server`, which then reads its write credential"
        }
    };
    let vouches = match queue.vouches {
        Vouches::Gesture => "the head at the gesture's time on the forge",
        Vouches::Seen => "the head herald first reads with the gesture",
        Vouches::Approved => {
            "the head herald first reads with the gesture, once one holding its least role approved it"
        }
    };
    format!(
        "The merge queue {name} merges into {} of {} on {} {runs}, with {method}, a gesture vouching for {vouches}, a push stamped up to {} before it counted as after it, a branch brought up to date by {}, {write}, {applied}. It reads with its source's read credential: {}.",
        queue.branch,
        queue.forge.repository,
        queue.forge.forge,
        Duration(queue.skew).written(),
        queue.forge.update.name(),
        queue.forge.reads
    )
}

/// Each merge queue of `config` said, and a gap for each reason one does
/// not start.
pub(super) fn queued(config: &Config, findings: &mut Findings) {
    let markers = config.store.reads_posts();
    for (name, queue) in &config.merge_queues {
        findings.notes.push(said(name, queue));
        let at = |key: &str| format!("merge_queues.{name}.{key}");
        let unstarted = [
            queue
                .method
                .is_none()
                .then(|| (at("method"), METHODLESS.to_owned())),
            queue
                .write
                .is_none()
                .then(|| (at("write"), UNWRITTEN.to_owned())),
            markers.then(|| ("store.kind".to_owned(), UNRECORDED.to_owned())),
            forge_less(queue).map(|why| (at("runs"), why)),
        ];
        for (key, why) in unstarted.into_iter().flatten() {
            findings
                .gaps
                .push(gap(&format!("the merge queue \"{name}\""), key, why));
        }
    }
}
