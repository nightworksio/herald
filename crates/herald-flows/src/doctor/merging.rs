//! What doctor says of each merge queue (ADR-0087): where it merges and
//! through whose queue, how, how a branch is brought up to date, its write
//! credential, where its steps are taken, what becomes of a pull request
//! it handed over once its gesture's maker lost the role, why one does not
//! start, and, online, what its forge says of its branch and the methods
//! its repository allows.

use herald_config::{Config, MergeQueue};
use herald_core::Duration;
use herald_core::merging::{AppliedBy, RoleLost, Runs, Updated, Vouches};
use herald_kinds::{Branched, Merging};
use herald_ports::DynHttp;

use super::{Findings, Gap, Layer, gap, listed};
use crate::prepare::{MergeReading, PreparedSource};

/// What the forge of each merge queue says of the queue's branch and
/// repository, asked through each source's `Http`: whether it has a merge
/// queue or merge train there that herald can hand a pull request to, by
/// which method, whether it lets a pull request merge there only through
/// it, and whether the repository allows the queue's method.
pub(super) async fn forge_said(sources: &[(&PreparedSource, &dyn DynHttp)]) -> Findings {
    let mut findings = Findings::default();
    let readings = sources
        .iter()
        .flat_map(|(source, http)| source.merging.iter().map(move |reading| (reading, *http)));
    for (reading, http) in readings {
        let name = reading.name.as_str();
        let what = format!("reading the merge queue \"{name}\"");
        let key = |key: &str| format!("merge_queues.{name}.{key}");
        let methods = if reading.method.is_some() {
            match reading.reader.methods(http).await {
                Ok(methods) => Some(methods),
                Err(error) => {
                    findings
                        .gaps
                        .push(gap(&what, key("method"), error.into_reason()));
                    None
                }
            }
        } else {
            None
        };
        if let Some(methods) = &methods {
            match crate::merging::allowed(reading.method, methods) {
                Ok(method) => findings.notes.push(format!(
                    "The merge queue {name}: the repository allows a merge by \"{}\".",
                    method.name()
                )),
                Err(why) => findings.gaps.push(gap(&what, key("method"), why)),
            }
        }
        if reading.queueless {
            continue;
        }
        match reading.reader.branch(http, &reading.branch).await {
            Err(error) => findings.gaps.push(gap(
                &what,
                format!("merge_queues.{name}"),
                error.into_reason(),
            )),
            Ok(branched) => branch_said(reading, (&branched, methods.as_ref()), &mut findings),
        }
    }
    findings
}

/// What doctor says of `branched`, the branch of the merge queue `reading`
/// reads, in a repository allowing `methods` where they were read.
fn branch_said(
    reading: &MergeReading,
    (branched, methods): (&Branched, Option<&herald_kinds::Methods>),
    findings: &mut Findings,
) {
    let (name, branch) = (reading.name.as_str(), reading.branch.as_str());
    let what = format!("reading the merge queue \"{name}\"");
    if reading.runs == Runs::Herald {
        if branched.queued.only {
            findings.gaps.push(gap(
                &what,
                format!("merge_queues.{name}.runs"),
                format!("the forge lets a pull request merge into {branch} only through its own merge queue or merge train, so it refuses herald's own merge; give runs = \"forge\" to hand each pull request to it"),
            ));
        }
        return;
    }
    let asked = (reading.runs, reading.method, reading.forge);
    let unhanded = crate::merging::unhanded(asked, branched, methods);
    let handed = unhanded.is_empty();
    for (key, why) in unhanded {
        findings
            .gaps
            .push(gap(&what, format!("merge_queues.{name}.{key}"), why));
    }
    if !handed {
        return;
    }
    findings.notes.push(match (branched.queue, branched.queued.method) {
        (Some(true), Some(method)) => format!(
            "The merge queue {name}: the forge has a merge queue or merge train on {branch}, merging by \"{}\".",
            method.name()
        ),
        (Some(true), None) => format!(
            "The merge queue {name}: the forge has a merge queue or merge train on {branch}."
        ),
        _ => format!(
            "The merge queue {name}: the forge would not say whether {branch} has a merge queue or merge train, as on a branch under classic protection; each pull request is handed to it all the same."
        ),
    });
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

/// Why `updated = "signed"` on `forge` with `signer` can never let a
/// gesture vouch for the head herald's own update made, where it cannot:
/// no forge but GitHub signs a commit it makes, and off github.com only a
/// `signer` names whose signature counts.
#[must_use]
pub fn unsigned(forge: &Merging, signer: Option<&str>) -> Option<&'static str> {
    if !MergeQueue::signs(forge.forge) {
        return Some(UNSIGNED_FORGE);
    }
    (!MergeQueue::proves_signed(forge, signer)).then_some(UNSIGNED_HOST)
}

/// Why `updated = "signed"` on herald's own queue `queue` can never let a
/// gesture vouch for the head herald's own update made, where it cannot.
fn unsigned_queue(queue: &MergeQueue) -> Option<&'static str> {
    if queue.runs != Runs::Herald || queue.updated != Updated::Signed {
        return None;
    }
    unsigned(&queue.forge, queue.signer.as_deref())
}

/// Why `updated = "signed"` never carries on GitLab or Forgejo.
const UNSIGNED_FORGE: &str = "updated = \"signed\" never lets a gesture vouch for the head herald's own update made here: GitLab's rebase rewrites the commits, and Forgejo's API cannot tell the instance's signature from an author's, so every update needs a new gesture; give updated = \"approved\" to let an approval of the updated head vouch for it, or updated = \"never\" to say so";

/// Why `updated = "signed"` never carries off github.com without a signer.
const UNSIGNED_HOST: &str = "updated = \"signed\" never lets a gesture vouch for the head herald's own update made here: off github.com herald knows no committer the instance signs its own commits for; give signer = the committer its web flow signs for, which you vouch is the instance's own and no user's, or updated = \"approved\" or \"never\"";

/// A gap for each merge queue of `config` whose steps `herald server`
/// alone takes, where `layer` does not run it, naming the `layers` that do.
pub(super) fn unstepped(config: &Config, layer: &Layer, layers: &[Layer], gaps: &mut Vec<Gap>) {
    if layer.steps {
        return;
    }
    let serving: Vec<&str> = layers
        .iter()
        .filter(|layer| layer.steps)
        .map(|layer| layer.name)
        .collect();
    let run = if serving.is_empty() {
        String::new()
    } else {
        format!("run herald on the {} layer, or ", listed(&serving, "or"))
    };
    let served = config
        .merge_queues
        .iter()
        .filter(|(_, queue)| queue.applied_by == AppliedBy::Server);
    for (name, _) in served {
        gaps.push(gap(
            &format!("the merge queue \"{name}\""),
            format!("merge_queues.{name}.applied_by"),
            format!(
                "applied_by = \"server\" leaves its steps to herald server, which does not run on the {} layer, so no step is ever taken here; {run}give applied_by = \"command\" and run herald merge-queue --apply on a schedule",
                layer.name
            ),
        ));
    }
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
        if let Some(why) = unsigned_queue(queue) {
            let feature = format!("the merge queue \"{name}\"");
            findings
                .gaps
                .push(gap(&feature, at("updated"), why.to_owned()));
        }
    }
}
