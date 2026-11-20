//! What doctor says of each report and rule: how its view reads each
//! source and what that costs a round, which routes are shown its private
//! rows, and where its times are read (ADR-0055); and of each living route
//! whose destination cannot edit (ADR-0056).

use std::fmt::Write as _;

use herald_config::{Config, Guard, Report, ViewConfig, ViewMode};
use herald_core::report::UTC;
use herald_core::stage_name;

/// What the time zone database bundled into this build adds to it, as a
/// report naming a zone reads it.
const TZDB_SIZE: &str =
    "the time zone database bundled into this build, about 207 KB (49 KB compressed)";

/// What doctor says of a view reading a source through GitHub's REST API
/// with no token.
const UNMERGEABLE: &str = "GitHub's REST API reads no mergeability, so none of its bots' batches counts green; give the source a token to read by GraphQL";

/// What doctor says of a view reading an owner's repositories by one query
/// over them (ADR-0062).
const SCOPED: &str = "it reads only the repositories the owner's membership record holds, each seen as that record says; where GitHub's search finds more than 1000, or the owner is a GitLab user, it lists each repository on its own, at least a request each";

/// What doctor says of a view taking who may see a repository from what a
/// REST listing read without a token states (ADR-0062).
const STATED: &str = "takes who may see each repository a REST listing reads without a token, on GitHub, GitHub Enterprise Server, GHE.com or Forgejo, from what the listing states, as listed_visibility = true asks; a read with a token, a GitLab listing, a kept record and an owner's rows are seen as before";

/// Each note on each view that sets `listed_visibility`.
pub(super) fn stated(config: &Config, notes: &mut Vec<String>) {
    notes.extend(
        config
            .views
            .iter()
            .filter(|(_, view)| view.stated)
            .map(|(name, _)| format!("The view {name} {STATED}.")),
    );
}

/// Each note on the reports and rules of `config`.
pub(super) fn reported(config: &Config, notes: &mut Vec<String>) {
    let reports = config
        .reports
        .iter()
        .map(|(name, report)| (format!("report {name}"), name, report));
    let rules = config
        .rules
        .iter()
        .map(|(name, rule)| (format!("rule {name}"), name, &rule.watch));
    let watching = reports.chain(rules).filter_map(|(named, name, report)| {
        Some((named, name, report, config.views.get(&report.view)?))
    });
    for (named, name, report, view) in watching {
        watched(config, (&named, name), (report, view), notes);
    }
    for (name, rule) in &config.rules {
        let stages: Vec<String> = rule
            .stages
            .as_slice()
            .iter()
            .map(|stage| stage_name(*stage))
            .collect();
        notes.push(format!(
            "The rule {name} nudges at most {} pull requests a window, once each past {}, naming logins without mentioning anyone.",
            rule.most,
            stages.join(", ")
        ));
    }
    for route in config
        .routes
        .iter()
        .filter(|route| route.route.delivery.living && !route.profile.features.edits)
    {
        notes.push(format!(
            "The route {} is living, but its destination cannot edit a post, so each window of its report is posted anew.",
            route.route.id.as_str()
        ));
    }
}

/// Each note on each view reading a source kept, and each view asking to
/// read kept of a store that keeps no pull requests (ADR-0061).
pub(super) fn kept(config: &Config, findings: &mut super::Findings) {
    let keeps = config.store.keeps_pulls();
    for (name, view) in &config.views {
        if view.mode == ViewMode::Kept && !keeps {
            findings.mistakes.push(format!(
                "The view {name} reads kept pull requests, and the store keeps none; choose a store that keeps them, or set its mode = \"live\"."
            ));
            continue;
        }
        let every = view.reconcile.map_or_else(
            || "every round".to_owned(),
            |every| format!("at most once every {every:#}"),
        );
        for source in &view.sources {
            if !crate::kept::reads_kept((config, keeps), view, source) {
                continue;
            }
            let mut note = format!(
                "The view {name} reads {} kept: from the records the store keeps, each webhook delivery's change folded in, its open pull requests listed again {every}, each page conditional on its last answer, and only those that changed read in full",
                source.as_str()
            );
            if !crate::kept::webhooked(config, source) {
                note.push_str(
                    "; the source takes no webhooks, so its records move only when it is listed",
                );
            }
            note.push('.');
            findings.notes.push(note);
        }
    }
}

/// Each note on the report or rule `name`, called `named`, watching
/// `view` as `report` says.
fn watched(
    config: &Config,
    (named, name): (&str, &str),
    (report, view): (&Report, &ViewConfig),
    notes: &mut Vec<String>,
) {
    let costs = view.sources.iter().filter_map(|source| {
        let options = config.sources.get(source)?;
        Some((source, options.queue_cost(view.most)?))
    });
    for (source, cost) in costs {
        let mut note = format!(
            "The {named} reads the view {} of {} by {}, at most {} requests a round",
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
        if cost.scoped {
            note.push_str("; ");
            note.push_str(SCOPED);
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
            "The {named} reads its times in {zone} from {TZDB_SIZE}."
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
                "The route {id} is not private, so it is shown the {named}'s rows of public repositories alone, and nothing where the view reads none."
            )),
            Guard::Warn => Some(format!(
                "The route {id} is not private and is shown the {named}'s rows of private and internal repositories too, as [doctor] private_to_public = \"warn\" lets it."
            )),
            Guard::Allow => None,
        });
    }
}
