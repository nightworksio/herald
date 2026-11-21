//! `herald rerun`: a rule's plans to re-run a flaky check listed, each with
//! the command that applies it; or the jobs named applied by an operator
//! signed in at the forge, each said as it ended (ADR-0068). Like `herald
//! queue`'s rows, a private or internal repository's plans are withheld
//! and counted unless the output is said private.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::Hosting;
use herald_compose::told::why;
use herald_config::{Config, Uses};
use herald_flows::{Applied, Done, Planned, PlansAsked, SigningIn};
use herald_native::TokioTimer;

use super::output::say;
use super::queueing::withheld_line;
use super::stores::composed;
use super::{Connect, DONE};

/// Why a job named is not as the plan prints it.
const UNREAD_JOB: &str = "name a job as the plan prints it, <job>@<head>";

/// `planned` as lines: the pull request, the check and its evidence, and
/// the run and its job with the exact call and the command applying it, or
/// why none.
pub(crate) fn planned_lines(planned: &Planned) -> Vec<String> {
    let mut lines = vec![format!(
        "{} {}#{}: {} flipped {} times; head {}",
        planned.rule,
        planned.repository,
        planned.number,
        planned.check,
        planned.flips,
        planned.head
    )];
    match &planned.run {
        Ok(rerun) => {
            lines.push(format!(
                "  run {} job {}  {}",
                rerun.run, rerun.job, rerun.page
            ));
            lines.push(format!("  call: {}", rerun.call));
            lines.push(format!(
                "  to apply: herald rerun {} --apply --job {}@{}",
                planned.rule, rerun.job, planned.head
            ));
        }
        Err(reason) => lines.push(format!("  nothing to re-run: {reason}")),
    }
    lines
}

/// What became of `applied`, on one line.
pub(crate) fn applied_line(applied: &Applied) -> String {
    let at = match (&applied.repository, applied.number) {
        (Some(repository), Some(number)) => format!("{repository}#{number} "),
        _ => String::new(),
    };
    let said = match &applied.done {
        Done::Rerun => format!("re-run: {}", herald_core::Reran::Rerun.said()),
        Done::Refused(reason) => format!("not re-run: {reason}"),
        Done::Declined(status) => format!(
            "not re-run: the forge refused the call with status {status}; a later apply may ask again"
        ),
        Done::Unsettled => format!("unsettled: {}", herald_core::Reran::Unsettled.said()),
        Done::Unrecorded(outcome) => format!(
            "{}; herald's record of it could not be updated, so it still says the call is being made and herald never asks for it again",
            outcome.said()
        ),
    };
    format!("{at}job {}@{}: {said}", applied.job, applied.head)
}

/// Each `<job>@<head>` of `named`, or the first that is not one.
fn chosen(named: &[String]) -> Result<Vec<(String, String)>, String> {
    named
        .iter()
        .map(|named| match named.split_once('@') {
            Some((job, head)) if !job.is_empty() && !head.is_empty() => {
                Ok((job.to_owned(), head.to_owned()))
            }
            _ => Err(format!("\"{named}\" is not a job; {UNREAD_JOB}")),
        })
        .collect()
}

pub(crate) async fn rerunning(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    (planning, apply, named): (PlansAsked<'_>, bool, &[String]),
    out: &mut dyn Write,
) -> Result<u8, String> {
    let chosen = chosen(named)?;
    let composed = composed(
        config,
        vars,
        connect,
        Hosting::Actions,
        Uses::Polls,
        "rerun",
    )
    .await?;
    if !apply {
        let plans = composed
            .rerun_plans(planning)
            .await
            .map_err(|stop| why(&stop))?;
        if plans.planned.is_empty() {
            say(
                out,
                "no flaky check is failing on the rule's open pull requests, so nothing is planned",
            );
        }
        for line in plans.planned.iter().flat_map(planned_lines) {
            say(out, &line);
        }
        if let Some(line) = withheld_line("the plans of ", plans.withheld) {
            say(out, &line);
        }
        return Ok(DONE);
    }
    let out = RefCell::new(out);
    let told = |signing: SigningIn<'_>| {
        say(
            *out.borrow_mut(),
            &format!(
                "To apply the re-runs of {}, open {} and enter the code {} within {:#}.",
                signing.source, signing.verification, signing.user_code, signing.expires
            ),
        );
    };
    let applied = composed
        .rerun_apply((planning, &chosen), &TokioTimer, &told)
        .await
        .map_err(|stop| why(&stop))?;
    let out = out.into_inner();
    for applied in &applied {
        say(out, &applied_line(applied));
    }
    Ok(DONE)
}
