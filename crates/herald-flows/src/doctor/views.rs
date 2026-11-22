//! What doctor says of each report and rule: how its view reads each
//! source and what that costs a round, which routes are shown its private
//! rows, and where its times are read (ADR-0055); and of each living route
//! whose destination cannot edit (ADR-0056); and what a view's advisories
//! cost, the permissions they need, and which routes are sent the
//! withheld line in their place (ADR-0067).

use std::fmt::Write as _;

use herald_config::{Config, Guard, Report, RouteConfig, Rule, ViewConfig, ViewMode};
use herald_core::report::UTC;
use herald_core::stage_name;
use herald_core::{Advising, AdvisoriesFrom, Capacity, Surface, View, WITHHELD, advises};
use herald_kinds::{Checking, UNADVISED};

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

/// What doctor says of a view predicting conflicts (ADR-0063).
const CAPACITY_PERMISSIONS: &str = "Its token needs Actions: read as a GitHub App or fine-grained token, repo (public_repo for a public repository) as a classic token, the same on GitHub Enterprise Server and GHE.com, read_api with the Reporter role on GitLab, self-managed included, and read access on Forgejo; a repository it may not read is named unread with why, never counted as idle.";

/// What doctor says a view's advisories need of its token (ADR-0067).
const ADVISORY_PERMISSIONS: &str = "Its token needs Dependabot alerts, Code scanning alerts and Secret scanning alerts: read as a GitHub App or fine-grained token, security_events (and repo for a private repository) as a classic token, and for an organization's own listings to act for an organization owner or security manager (with read:org as a classic token), the same on GitHub Enterprise Server and GHE.com; read_api with the Developer role on GitLab Ultimate, self-managed included. A listing it may not read is named unread with the permission it needs, never shown as clean. Forgejo raises no security alerts, so its repositories are named unread.";

const PATHS: &str = "it reads the paths each pull request's head changes, at most 100 each,";

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
        if let Some(stages) = &rule.stages {
            let stages: Vec<String> = stages
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
        if rule.rerun {
            planning(config, (name, rule), notes);
        }
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

/// Each note on the rule `name` planning re-runs: what it plans and where
/// it announces them, a view source read live, which names no flaky
/// candidate, and a forge that cannot re-run (ADR-0065).
fn planning(config: &Config, (name, rule): (&str, &Rule), notes: &mut Vec<String>) {
    let private = config.routes.iter().any(|route| {
        route.private
            && route
                .route
                .filter
                .sources
                .iter()
                .any(|source| source.as_str() == name)
    });
    let reach = if private {
        "to its private routes alone"
    } else {
        "to its private routes, and no private route names it, so its plans reach nobody"
    };
    notes.push(format!(
        "The rule {name} plans at most {} re-runs of flaky checks a window, each once a run and head with its evidence and exact call, {reach}; a rule never re-runs. A plan reads the newest {} failed runs on a head and each one's jobs: at most {} requests, each charged to the run's budget.",
        rule.most,
        rule.runs,
        rule.runs + 1
    ));
    let keeps = config.store.keeps_pulls();
    let live = config
        .views
        .get(&rule.watch.view)
        .into_iter()
        .flat_map(|view| {
            view.sources
                .iter()
                .filter(move |source| !crate::kept::reads_kept((config, keeps), view, source))
        });
    notes.extend(live.map(|source| {
        format!(
            "The rule {name} reads {} live, and only kept records tell a flaky candidate, so none of its checks is one.",
            source.as_str()
        )
    }));
    let sources = config
        .views
        .get(&rule.watch.view)
        .into_iter()
        .flat_map(|view| &view.sources)
        .filter_map(|source| config.sources.get(source).map(|found| (source, found)));
    for (source, found) in sources {
        notes.push(applies(config, (source.as_str(), found)));
    }
}

/// Whether `herald rerun --apply` can re-run the jobs of the source
/// `name`, and why not where it cannot (ADR-0068).
fn applies(config: &Config, (name, source): (&str, &herald_kinds::Source)) -> String {
    let unable =
        |why: &str| format!("herald rerun --apply cannot re-run the jobs {name} reads: {why}.");
    let Some(write) = source.write() else {
        return unable(herald_kinds::UNWRITTEN);
    };
    let signing = match source.signing() {
        Ok(signing) => signing,
        Err(why) => return unable(why),
    };
    if !config.store.keeps_actions() {
        return unable(crate::applying::UNKEPT);
    }
    let scopes = if signing.scopes.is_empty() {
        "no scope".to_owned()
    } else {
        signing.scopes.join(", ")
    };
    format!(
        "herald rerun --apply can re-run the jobs {name} reads, each once: an operator signs in at {} as the client {} asking {scopes}, and must hold {} or above on the repository; the call is made with {} alone, sent to the source's own API. A stolen write credential re-runs without herald, so give it nothing but re-running: a GitHub App installation with actions: write alone, or a GitLab token of the Developer role on these projects alone. The forge's audit log names that credential, not the operator; herald's record of each re-run names the operator.",
        signing.web,
        signing.client_id,
        signing.least.name(),
        write.name()
    )
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

/// What doctor says of the view `view`, which the report or rule called
/// `named` watches, reading capacity of the repositories `capacity` names
/// of those `held` holds (ADR-0066).
fn capacity_note((named, view): (&str, &str), (capacity, held): (Capacity, &View)) -> String {
    let which = match capacity {
        Capacity::Shown => "the repositories it shows a pull request of, so the runs of one with no open pull request are not counted".to_owned(),
        Capacity::Members { most } if !held.holds_every() => format!(
            "the first {most} by name of the members it holds, with no request for their order: it filters its repositories by name, so the push times of those it hides choose nothing"
        ),
        Capacity::Members { most } => format!(
            "its owner's {most} newest pushed members, by one more request for their order (on Forgejo, which lists no push order, the first {most} its membership record keeps)"
        ),
    };
    format!(
        "The {named} shows the view {view}'s capacity of {which}: two requests a repository on GitHub and GitLab, one on Forgejo. {CAPACITY_PERMISSIONS}"
    )
}

/// What doctor says of the view `view`, which the report or rule called
/// `named` watches, reading advisories as `advising` says (ADR-0067).
fn advisories_note((named, view): (&str, &str), (advising, held): (Advising, &View)) -> String {
    let (most, repositories) = (advising.most, advising.repositories);
    let read = advising.within(held);
    let which = match read.from {
        AdvisoriesFrom::Members if !held.holds_every() => format!(
            "the first {repositories} by name of the members it holds, one by one, with no request for their order: it filters its repositories by name, so it never reads its owner's own listings, whose pages count the alerts of the repositories it hides too, nor their push order"
        ),
        AdvisoriesFrom::Owner => format!(
            "its organization's or group's own listings, each advisory kept only where the membership record holds its repository; a GitHub user or GitLab user namespace has none, so its {repositories} newest pushed members are read one by one, by one more request for their order"
        ),
        AdvisoriesFrom::Shown => "the repositories it shows a pull request of, so one with no open pull request is not read".to_owned(),
        AdvisoriesFrom::Members => format!(
            "its owner's {repositories} newest pushed members, by one more request for their order"
        ),
    };
    format!(
        "The {named} reads the view {view}'s advisories of {which}; a one-repository source reads its repository. Each repository or owner read costs three requests on GitHub, one a listing, and one on GitLab, and one more for each further page of 100, at most {most} advisories read. They reach only routes that say private = true, and herald queue only with --private, typed each time with no setting or variable for it: in a public CI job's log --private publishes them; before each run's first advisory send, each such route's place is checked as its own note says, by requests charged to the run's budget. {ADVISORY_PERMISSIONS}"
    )
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
        let zoned = !options.area_names().is_empty() || options.codeowned();
        Some((source, options.queue_cost(view.most)?, zoned))
    });
    for (source, cost, zoned) in costs {
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
        let reading = match (view.conflicts, zoned) {
            (true, false) => Some("to predict conflicts"),
            (true, true) => Some("to predict conflicts and name each one's areas"),
            (false, true) => Some("to name each pull request's areas"),
            (false, false) => None,
        };
        if let Some(reading) = reading {
            let _ = match cost.files {
                0 => write!(note, "; {reading} {PATHS} with each pull request"),
                more => write!(
                    note,
                    "; {reading} {PATHS} by at most {more} more requests a round"
                ),
            };
        }
        note.push('.');
        notes.push(note);
    }
    notes.extend(
        view.capacity
            .map(|capacity| capacity_note((named, &report.view), (capacity, &view.view))),
    );
    notes.extend(
        view.advisories
            .map(|advising| advisories_note((named, &report.view), (advising, &view.view))),
    );
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
    let naming: Vec<&RouteConfig> = config
        .routes
        .iter()
        .filter(|route| {
            route
                .route
                .filter
                .sources
                .iter()
                .any(|source| source.as_str() == name)
        })
        .collect();
    if view.advisories.is_some() && config.reports.contains_key(name) {
        advised_notes(config, named, &naming, notes);
    }
    for route in naming.into_iter().filter(|route| !route.private) {
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

/// What doctor says of how herald learns who may read the place of the
/// private route `id`, which `checking` says and the operator may have
/// `vouched` for, before the report `named` sends it advisories.
fn checked_note((id, named): (&str, &str), checking: Checking, vouched: bool) -> String {
    match checking {
        Checking::Asked { asks, requests } => format!(
            "Before each run's first advisory send, herald asks {asks}, by {requests} requests charged to the run's budget, once a run for its place; where anyone more may read it, or herald cannot tell, the route {id} is sent \"{WITHHELD}\" in place of the {named}'s advisories."
        ),
        Checking::Unchecked(why) if vouched => format!(
            "The route {id} is sent the {named}'s advisories on the operator's word alone, as it says vouch_unchecked = true: {why}."
        ),
        Checking::Unchecked(why) => format!(
            "The route {id} says private = true, but {why}; it is sent \"{WITHHELD}\" in place of the {named}'s advisories, unless you vouch for its place with vouch_unchecked = true."
        ),
        Checking::Unadvised => format!(
            "The route {id} is sent \"{WITHHELD}\" in place of the {named}'s advisories: {UNADVISED}."
        ),
    }
}

/// Each note on the routes `naming` the report `named` whose view shows
/// advisories: each not shown them is sent the withheld line, and two that
/// post to one place where only one is shown them are flagged, since all
/// who read the one read the other (ADR-0067).
fn advised_notes(config: &Config, named: &str, naming: &[&RouteConfig], notes: &mut Vec<String>) {
    for route in naming {
        let id = route.route.id.as_str();
        let surface = config.surface(&route.route.id);
        if matches!(
            surface,
            Surface::Route {
                private: true,
                recallable: false
            }
        ) {
            notes.push(format!(
                "The route {id} says private = true, but its post is not living, so it is sent \"{WITHHELD}\" in place of the {named}'s advisories: a report carries its advisories only in a living post (living = true, on a destination that edits), which herald can always find again to take the section out."
            ));
        } else if !advises(surface) {
            notes.push(format!(
                "The route {id} is not private, or posts where anyone may read it, so it is sent \"{WITHHELD}\" in place of the {named}'s advisories, whatever [doctor] private_to_public says."
            ));
        } else if route.route.delivery.living && !config.store.keeps_recalls() {
            notes.push(format!(
                "The route {id} keeps a living post and says private = true, but {}, so it is sent \"{WITHHELD}\" in place of the {named}'s advisories.",
                crate::recall::UNINDEXED
            ));
        } else if let Some(checking) = config
            .destinations
            .get(&route.route.destination)
            .map(|destination| destination.checking(&route.options))
        {
            notes.push(checked_note((id, named), checking, route.vouched));
        }
    }
    for (index, shown) in naming.iter().enumerate() {
        for other in naming.iter().skip(index + 1) {
            let place = |route: &RouteConfig| {
                route
                    .options
                    .written()
                    .map(|written| (route.route.destination.clone(), written))
            };
            let both = advises(config.surface(&shown.route.id))
                != advises(config.surface(&other.route.id));
            if both && place(shown).is_some() && place(shown) == place(other) {
                notes.push(format!(
                    "The routes {} and {} post to one place, and only one of them is shown the {named}'s advisories: everyone who reads the other reads them there; give the private one a place of its own.",
                    shown.route.id.as_str(),
                    other.route.id.as_str(),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests;
