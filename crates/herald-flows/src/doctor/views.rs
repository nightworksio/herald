//! What doctor says of each report: how its view reads each source and
//! what that costs a round, which routes are shown its private rows, and
//! where its times are read (ADR-0055).

use std::fmt::Write as _;

use herald_config::{Config, Guard};
use herald_core::report::UTC;

/// What the time zone database bundled into this build adds to it, as a
/// report naming a zone reads it.
const TZDB_SIZE: &str =
    "the time zone database bundled into this build, about 207 KB (49 KB compressed)";

/// What doctor says of a view reading a source through GitHub's REST API
/// with no token.
const UNMERGEABLE: &str = "GitHub's REST API reads no mergeability, so none of its bots' batches counts green; give the source a token to read by GraphQL";

/// Each note on the reports of `config`.
pub(super) fn reported(config: &Config, notes: &mut Vec<String>) {
    let reports = config
        .reports
        .iter()
        .filter_map(|(name, report)| Some((name, report, config.views.get(&report.view)?)));
    for (name, report, view) in reports {
        let costs = view.sources.iter().filter_map(|source| {
            let options = config.sources.get(source)?;
            Some((source, options.queue_cost(view.most)?))
        });
        for (source, cost) in costs {
            let mut note = format!(
                "The report {name} reads the view {} of {} by {}, at most {} requests a round",
                report.view,
                source.as_str(),
                cost.path,
                cost.requests
            );
            if cost.detailed {
                let _ = write!(
                    note,
                    "; its open pull requests past the first {} read as unknown",
                    view.most.pulls
                );
            }
            if !cost.mergeability {
                note.push_str("; ");
                note.push_str(UNMERGEABLE);
            }
            note.push('.');
            notes.push(note);
        }
        if let Some(zone) = report
            .schedule
            .zone
            .iana_name()
            .filter(|zone| !zone.eq_ignore_ascii_case(UTC))
        {
            notes.push(format!(
                "The report {name} reads its times in {zone} from {TZDB_SIZE}."
            ));
        }
        for route in config.routes.iter().filter(|route| {
            !route.private
                && route
                    .route
                    .filter
                    .sources
                    .iter()
                    .any(|source| source.as_str() == name)
        }) {
            let id = route.route.id.as_str();
            notes.extend(match config.doctor.private_to_public {
                Guard::Refuse => Some(format!(
                    "The route {id} is not private, so it is shown the report {name}'s rows of public repositories alone, and nothing where the view reads none."
                )),
                Guard::Warn => Some(format!(
                    "The route {id} is not private and is shown the report {name}'s rows of private and internal repositories too, as [doctor] private_to_public = \"warn\" lets it."
                )),
                Guard::Allow => None,
            });
        }
    }
}
