//! What doctor says of each merge queue (ADR-0087): where it merges and
//! through whose queue, how, how a branch is brought up to date, its write
//! credential, where its steps are taken, what becomes of a pull request
//! it handed over once its gesture's maker lost the role, and why one does
//! not start.

use herald_config::{Config, MergeQueue};
use herald_core::Duration;
use herald_core::merging::{AppliedBy, RoleLost, Runs, Vouches};

use super::{Findings, gap};

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
    let role_lost = match (queue.runs, queue.role_lost) {
        (Runs::Forge, RoleLost::TakeOut) => {
            ", a pull request it handed over taken out of the forge's queue once its gesture's maker no longer holds its least role"
        }
        (Runs::Forge, RoleLost::Stay) => {
            ", a pull request it handed over left in the forge's queue once its gesture's maker no longer holds its least role"
        }
        (Runs::Herald, _) => "",
    };
    let vouches = match queue.vouches {
        Vouches::Gesture => "the head at the gesture's time on the forge",
        Vouches::Seen => "the head herald first reads with the gesture",
        Vouches::Approved => {
            "the head herald first reads with the gesture, once one holding its least role approved it"
        }
    };
    format!(
        "The merge queue {name} merges into {} of {} on {} {runs}, with {method}, a gesture vouching for {vouches}, a push stamped up to {} before it counted as after it, a branch brought up to date by {}, {write}, {applied}{role_lost}. It reads with its source's read credential: {}.",
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
        for (key, why) in crate::merging::unstarted(queue, markers) {
            let key = if key == "store" {
                "store.kind".to_owned()
            } else {
                at(key)
            };
            findings
                .gaps
                .push(gap(&format!("the merge queue \"{name}\""), key, why));
        }
    }
}
