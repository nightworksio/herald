//! `herald orphans clear`: the possibly orphaned parts of a report's living
//! posts, which doctor names, listed with the command that clears each
//! route's, and one route's dropped once the operator says the parts they
//! were shown were deleted by hand (ADR-0067).

use std::collections::BTreeMap;
use std::io::Write;

use herald_compose::Hosting;
use herald_config::{Config, Uses};
use herald_core::RouteId;
use herald_flows::Orphaned;

use super::output::say;
use super::stores::composed;
use super::{Connect, DONE};

/// What the listing ends with: what clearing means.
const CLEARING: &str = "Clearing a route's parts with the command shown makes herald stop tracking them: any part that still exists may go on showing an advisories section, possibly in a public place, and herald will not recall it.";

/// The parts of `orphaned`, each once, with how many times it was sent
/// under a key of its own where more than once.
fn described(orphaned: &Orphaned) -> String {
    let mut sent: BTreeMap<u32, usize> = BTreeMap::new();
    for part in &orphaned.parts {
        *sent.entry(*part).or_default() += 1;
    }
    sent.into_iter()
        .map(|(part, times)| {
            let named = if part == 0 {
                "the post".to_owned()
            } else {
                format!("chunk {part}")
            };
            if times > 1 {
                format!("{named} (sent {times} times)")
            } else {
                named
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The line naming what `orphaned` holds of the report's living post.
fn line(said: &str, report: &str, orphaned: &Orphaned) -> String {
    format!(
        "{said}: {} of the report {report}'s living post on the route {}",
        described(orphaned),
        orphaned.route.as_str()
    )
}

/// Each possibly orphaned part of the report's living posts, of one route
/// where `route` names it, printed with the command that clears each
/// route's; or, where the operator names the digest they were shown as
/// `seen`, that route's dropped; or why not.
pub(crate) async fn cleared(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    (report, route, seen): (&str, Option<&str>, Option<&str>),
    out: &mut dyn Write,
) -> Result<u8, String> {
    let composed = composed(
        config,
        vars,
        connect,
        Hosting::Actions,
        Uses::Polls,
        "orphans",
    )
    .await?;
    let route = route.map(RouteId::new);
    if let (Some(route), Some(seen)) = (&route, seen) {
        let cleared = composed.orphans_cleared(report, (route, seen)).await?;
        say(out, &line("cleared", report, &cleared));
        return Ok(DONE);
    }
    for orphaned in composed.orphans(report, route.as_ref()).await? {
        say(out, &line("possibly orphaned", report, &orphaned));
        say(
            out,
            &format!(
                "  once deleted by hand: herald orphans clear {report} --route {} --deleted --seen {}",
                orphaned.route.as_str(),
                orphaned.seen
            ),
        );
    }
    say(out, CLEARING);
    Ok(DONE)
}
