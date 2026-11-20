//! A rule's plan to re-run a flaky check once: the pull request whose
//! check failed, the evidence that it is flaky, and the exact call that
//! would re-run the job of that check, or why none can (ADR-0065,
//! ADR-0068).

use std::collections::BTreeMap;

use crate::event::{Draft, Event, Problem};
use crate::kind::{
    Attribute, CALL, CHECK, FLIPS, HEAD, Kind, NUMBER, PULL_LINK, REASON, REPOSITORY, RULE, RUN_ID,
    RUN_LINK,
};
use crate::nudge::pull_identity;
use crate::queue::Pull;

/// The most failed runs on one head a plan reads for one check where its
/// rule names no `runs`, the newest first.
pub const MOST_RUNS: usize = 5;

/// The most failed runs on one head a rule may have a plan read, one page
/// of every forge's listing.
pub const MOST_RUNS_ALLOWED: usize = 100;

/// What an event's identity begins with where it is a re-run plan.
pub const RERUN_PREFIX: &str = "rerun";

/// One job a plan would re-run: its run's forge id and its own, the run's
/// page, and the exact call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rerun {
    /// The run's id on its forge.
    pub run: String,
    /// The id of the run's job of the check, the one job re-run.
    pub job: String,
    /// The run's page.
    pub page: String,
    /// The exact call that re-runs that job alone, as `METHOD URL`.
    pub call: String,
}

/// A rule's plan for one flaky check failing on one pull request.
#[derive(Debug, Clone, Copy)]
pub struct RerunPlan<'a> {
    /// The pull request.
    pub pull: &'a Pull,
    /// The head commit it failed on.
    pub head: &'a str,
    /// The check.
    pub check: &'a str,
    /// How many times it failed then passed on one head within the view's
    /// window.
    pub flips: usize,
    /// The job it would re-run, with its run, or why none can be.
    pub run: Result<&'a Rerun, &'a str>,
}

/// The event of rule `rule`'s `plan`, seen as the pull request's repository
/// is, its identity naming the run and head so it is announced once.
///
/// # Errors
///
/// Where the pull request's project is empty, or its title holds nothing
/// but space.
pub fn rerun_event(rule: &str, plan: RerunPlan<'_>) -> Result<Event, Problem> {
    let pull = plan.pull;
    let head = plan.head;
    let text = |text: &str| Attribute::Text(text.to_owned());
    let mut attributes = BTreeMap::from([
        (RULE.to_owned(), text(rule)),
        (CHECK.to_owned(), text(plan.check)),
        (HEAD.to_owned(), text(head)),
        (FLIPS.to_owned(), Attribute::Number(plan.flips.into())),
        (REPOSITORY.to_owned(), text(&pull.repository)),
        (NUMBER.to_owned(), Attribute::Number(pull.number.into())),
    ]);
    let mut links = BTreeMap::from([(PULL_LINK.to_owned(), pull.url.clone())]);
    let run = match plan.run {
        Ok(rerun) => {
            attributes.insert(RUN_ID.to_owned(), text(&rerun.run));
            attributes.insert(CALL.to_owned(), text(&rerun.call));
            links.insert(RUN_LINK.to_owned(), rerun.page.clone());
            rerun.run.as_str()
        }
        Err(reason) => {
            attributes.insert(REASON.to_owned(), text(reason));
            "none"
        }
    };
    let event = Event::normalise(Draft {
        identity: format!(
            "{RERUN_PREFIX}/{rule}/{}/{}/{head}/{run}",
            pull_identity(pull),
            plan.check
        ),
        kind: Kind::RERUN,
        project: pull.project.clone(),
        title: pull.title.clone(),
        body: String::new(),
        links,
        attributes,
        occurred: pull.updated,
    })?;
    Ok(event.seen_as(pull.visibility))
}

#[cfg(test)]
mod tests;
