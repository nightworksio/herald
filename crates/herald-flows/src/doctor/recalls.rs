//! Each living post a report's cursor keeps whose route no longer receives
//! its advisories section, which herald edits out at the next report
//! round, and names here until it does, for every report the store names
//! whether or not the config still holds it; and each such post a part of
//! which herald sent and never found again, or whose deletion and post
//! anew a run left part way (ADR-0067).

use herald_config::Config;
use herald_core::{AdvisedPost, SourceName};
use herald_ports::{Cursors, Ledger, Recalls};

use super::Findings;
use crate::recall::{living_key, receives};

/// What the ledger `ledger` shows of `post` of the report `name`: a
/// mistake where a part of it was sent and never found again, and a note
/// where a run left its deletion and post anew part way.
async fn ledgered<L: Ledger>(ledger: &L, name: &str, post: &AdvisedPost, found: &mut Findings) {
    let (route, destination) = (post.route.as_str(), post.destination.as_str());
    let entry = match ledger.entry(&living_key(name, post)).await {
        Ok(entry) => entry,
        Err(error) => {
            found.mistakes.push(format!(
                "the ledger entry of the living post of the report {name} on the route {route} could not be read, so doctor cannot say whether a part of it still shows advisories: {error}"
            ));
            return;
        }
    };
    let Some(entry) = entry else { return };
    if !entry.value.strays.is_empty() {
        found.mistakes.push(format!(
            "the living post of the report {name} on the route {route} through {destination} may have left an orphaned part: herald sent it and could not find it again, so it may still show an advisories section; look for it in that place, delete it by hand, then run `herald orphans clear {name} --route {route}` and the command it prints to confirm"
        ));
    }
    if entry.value.replacing.is_some() {
        found.notes.push(format!(
            "the living post of the report {name} on the route {route} through {destination} is being deleted and posted anew, and a run stopped part way, so the place may still show what it showed until a run finishes it; each run goes on with it before it sends any section"
        ));
    }
}

/// Each living post the cursor of a report `recalls` names, read from
/// `cursors`, keeps that its route no longer receives the section in: a
/// mistake naming the report, the route and the destination, because the
/// post shows the section until herald edits it out; each such post the
/// ledger `ledger` shows a part of sent and never found again, or a
/// deletion and post anew left part way; the names or a cursor that cannot
/// be read named too.
pub async fn recalls<C: Cursors, R: Recalls, L: Ledger>(
    config: &Config,
    (cursors, recalls, ledger): (&C, &R, &L),
) -> Findings {
    let mut findings = Findings::default();
    let named = match recalls.advised().await {
        Ok(named) => named.map(|named| named.value.reports).unwrap_or_default(),
        Err(error) => {
            findings.mistakes.push(format!(
                "the names of the reports whose living posts were sent advisories could not be read, so doctor cannot say whether a living post still shows advisories its route no longer receives: {error}"
            ));
            return findings;
        }
    };
    for name in &named {
        let record = match cursors.cursor(&SourceName::new(name)).await {
            Ok(record) => record,
            Err(error) => {
                findings.mistakes.push(format!(
                    "the report {name}'s cursor could not be read, so doctor cannot say whether a living post still shows advisories its route no longer receives: {error}"
                ));
                continue;
            }
        };
        for post in record.iter().flat_map(|record| &record.value.advised) {
            ledgered(ledger, name, post, &mut findings).await;
        }
        for post in record
            .iter()
            .flat_map(|record| &record.value.advised)
            .filter(|post| !receives(config, name, post))
        {
            findings.mistakes.push(format!(
                "the living post of the report {name} on the route {} through {} still shows an advisories section its route no longer receives; herald edits it to one fixed line at each report round until that lands, and where its destination is gone or cannot edit it, delete the post by hand",
                post.route.as_str(),
                post.destination.as_str(),
            ));
        }
    }
    findings
}
