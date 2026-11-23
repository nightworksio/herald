//! A repository's CI runs, each announced as the `run` signal: GitHub
//! Actions' workflow runs, GitLab's pipelines and Forgejo Actions' runs,
//! and, where a source reads checks, GitHub's check suites and commit
//! statuses.

use std::collections::BTreeMap;

use herald_core::{
    ACTION, ACTOR, ATTEMPT, Attribute, BRANCH, COMMIT, CONCLUSION, Cursor, Draft, Event, FROM,
    Kind, NUMBER, Polled, RUN_LINK, SIGNAL, Signal, TRIGGER, Validators, WORKFLOW,
};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Request, Response};
use herald_wire::SourceError;
use jiff::Timestamp;

use crate::alerts;
use crate::forge::{Repository, validators};
use crate::options::RunChoice;
use crate::{Conclusion, Forge, Origin, RunAction, RunFrom, Token};

mod forgejo;
mod github;
mod gitlab;

/// What a run is on its forge, as its identity names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ran {
    /// A workflow run, pipeline or Actions run.
    Run,
    /// A GitHub check suite.
    CheckSuite,
    /// A GitHub commit status.
    Status,
}

impl Ran {
    const fn name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::CheckSuite => "check-suite",
            Self::Status => "status",
        }
    }

    const fn from(self) -> RunFrom {
        match self {
            Self::Run => RunFrom::Actions,
            Self::CheckSuite | Self::Status => RunFrom::Checks,
        }
    }
}

/// A run, as any forge describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeRun {
    pub(crate) ran: Ran,
    pub(crate) id: u64,
    pub(crate) attempt: Option<u64>,
    pub(crate) number: Option<u64>,
    pub(crate) workflow: String,
    pub(crate) action: RunAction,
    pub(crate) conclusion: Option<Conclusion>,
    pub(crate) trigger: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) commit: Option<String>,
    pub(crate) actor: Option<String>,
    pub(crate) url: String,
    pub(crate) title: Option<String>,
    pub(crate) occurred: Timestamp,
}

/// What a run's title says it did.
const fn did(action: RunAction, conclusion: Option<Conclusion>) -> &'static str {
    match (action, conclusion) {
        (RunAction::Requested, _) => "was requested",
        (RunAction::InProgress, _) => "started",
        (RunAction::Completed, Some(Conclusion::Success)) => "succeeded",
        (RunAction::Completed, Some(Conclusion::Failure)) => "failed",
        (RunAction::Completed, Some(Conclusion::Cancelled)) => "was cancelled",
        (RunAction::Completed, Some(Conclusion::TimedOut)) => "timed out",
        (RunAction::Completed, Some(Conclusion::Skipped)) => "was skipped",
        (RunAction::Completed, Some(Conclusion::ActionRequired)) => "needs action",
        (RunAction::Completed, Some(Conclusion::StartupFailure)) => "failed to start",
        (RunAction::Completed, Some(Conclusion::Stale)) => "went stale",
        (RunAction::Completed, Some(Conclusion::Neutral) | None) => "finished",
    }
}

impl ForgeRun {
    /// The last part of its identity: its attempt where its forge counts
    /// them, or else how it ended, so a run its forge runs again announces
    /// again only where it ends otherwise; and what it did, where it has
    /// not finished.
    fn attempted(&self) -> String {
        let finished = match self.conclusion {
            Some(conclusion) => conclusion.name(),
            None => self.action.name(),
        };
        match (self.attempt, self.action) {
            (Some(attempt), RunAction::Completed) => attempt.to_string(),
            (Some(attempt), action) => format!("{attempt}/{}", action.name()),
            (None, RunAction::Completed) => finished.to_owned(),
            (None, action) => action.name().to_owned(),
        }
    }

    /// Whether `choice` announces it.
    pub(crate) fn chosen(&self, choice: &RunChoice) -> bool {
        choice.reads(self.ran.from())
            && choice.takes(
                (self.action, self.conclusion),
                (
                    self.branch.as_deref(),
                    self.trigger.as_deref(),
                    &self.workflow,
                ),
            )
    }

    /// The event it makes as a run of `repository`, named on the host of
    /// the repository's page, or else of its own.
    pub(crate) fn event(
        self,
        what: &'static str,
        repository: &Repository,
    ) -> Result<Event, SourceError> {
        let host = crate::wire::host_named(what, repository, &self.url)?;
        let identity = format!(
            "{host}/{}/{}/{}/{}",
            repository.id,
            self.ran.name(),
            self.id,
            self.attempted()
        );
        let on = self
            .branch
            .as_deref()
            .map_or_else(String::new, |branch| format!(" on {branch}"));
        let title = format!(
            "{} {}{on}",
            self.workflow,
            did(self.action, self.conclusion)
        );
        let text = |value: &str| Attribute::Text(value.to_owned());
        let mut attributes = BTreeMap::from([
            (SIGNAL.to_owned(), text(Signal::Run.name())),
            (ACTION.to_owned(), text(self.action.name())),
            (WORKFLOW.to_owned(), text(&self.workflow)),
            (FROM.to_owned(), text(self.ran.from().name())),
        ]);
        let named = [
            (
                CONCLUSION,
                self.conclusion
                    .map(|conclusion| conclusion.name().to_owned()),
            ),
            (TRIGGER, self.trigger),
            (BRANCH, self.branch),
            (COMMIT, self.commit),
            (ACTOR, self.actor),
        ];
        for (name, value) in named {
            if let Some(value) = value {
                attributes.insert(name.to_owned(), Attribute::Text(value));
            }
        }
        for (name, value) in [(NUMBER, self.number), (ATTEMPT, self.attempt)] {
            if let Some(value) = value {
                attributes.insert(name.to_owned(), Attribute::Number(value.into()));
            }
        }
        let draft = Draft {
            identity,
            kind: Kind::RUN,
            project: repository.name.clone(),
            title,
            body: self.title.unwrap_or_default(),
            links: BTreeMap::from([(RUN_LINK.to_owned(), self.url)]),
            attributes,
            occurred: self.occurred,
        };
        Event::normalise(draft).map_err(|problem| SourceError::Invalid { what, problem })
    }
}

/// `path` asking for each of `asked` that has a value, as a query
/// parameter after those it holds.
pub(crate) fn queried(mut path: String, asked: &[(&str, Option<&str>)]) -> String {
    for (key, value) in asked {
        if let Some(value) = value {
            path.push('&');
            path.push_str(key);
            path.push('=');
            path.push_str(&herald_core::unreserved(value));
        }
    }
    path
}

/// The action a status, as GitHub names a workflow run's or a check
/// suite's, is of.
pub(crate) fn github_action(status: &str) -> RunAction {
    match status {
        "completed" => RunAction::Completed,
        "in_progress" => RunAction::InProgress,
        _ => RunAction::Requested,
    }
}

/// What a webhook delivery or a CI run's event is, where it is of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// GitHub's `workflow_run`, GitLab's `Pipeline Hook`, or Forgejo's
    /// `workflow_run_*` and `action_run_*`.
    Run,
    /// GitHub's `check_suite`.
    CheckSuite,
    /// GitHub's `status`.
    Status,
}

impl Delivery {
    /// What the event named `name` on `forge` is, where it is of a run.
    pub(crate) fn named(forge: Forge, name: &str) -> Option<Self> {
        match forge {
            Forge::GitHub => match name {
                "workflow_run" => Some(Self::Run),
                "check_suite" => Some(Self::CheckSuite),
                "status" => Some(Self::Status),
                _ => None,
            },
            Forge::GitLab => (name == "Pipeline Hook").then_some(Self::Run),
            Forge::Forgejo => forgejo::hooked(name).then_some(Self::Run),
        }
    }
}

/// What a run's webhook delivery or CI event names: the repository's
/// full path, and the run where it is one announced.
pub(crate) struct Delivered {
    pub(crate) path: String,
    pub(crate) repository: Repository,
    pub(crate) run: Option<ForgeRun>,
}

impl Forge {
    /// What the webhook or CI event names that `headers` or the run's
    /// event name name, where it is of a run.
    pub(crate) fn run_delivery(self, headers: &[(&str, &str)]) -> Option<Delivery> {
        self.event_named(headers)
            .and_then(|name| Delivery::named(self, name))
    }

    /// What the alert webhook `headers` name, where it is of an alert.
    pub(crate) fn alert_delivery(self, headers: &[(&str, &str)]) -> Option<alerts::Delivery> {
        self.event_named(headers)
            .and_then(|name| alerts::Delivery::named(self, name))
    }

    /// Whether the webhook `headers` name a pull request's event.
    pub(crate) fn pull_delivery(self, headers: &[(&str, &str)]) -> bool {
        self.event_named(headers)
            .is_some_and(|name| crate::pulls::hooked(self, name))
    }

    /// The name of the issue event the webhook `headers` deliver, where
    /// they deliver one.
    pub(crate) fn issue_event<'h>(self, headers: &[(&str, &'h str)]) -> Option<&'h str> {
        self.event_named(headers)
            .filter(|name| crate::issues::hooked(self, name))
    }

    /// Whether the webhook `headers` name a milestone's event.
    pub(crate) fn milestone_delivery(self, headers: &[(&str, &str)]) -> bool {
        self.event_named(headers)
            .is_some_and(|name| crate::milestones::hooked(self, name))
    }

    /// The name of the event a webhook's `headers` deliver.
    pub(crate) fn event_named<'h>(self, headers: &[(&str, &'h str)]) -> Option<&'h str> {
        let names: &[&str] = match self {
            Self::GitHub => &["x-github-event"],
            Self::GitLab => &["x-gitlab-event"],
            Self::Forgejo => &["x-forgejo-event", "x-gitea-event"],
        };
        headers
            .iter()
            .find(|(name, _)| names.iter().any(|header| name.eq_ignore_ascii_case(header)))
            .map(|(_, value)| *value)
    }

    /// What the run `delivery`'s `payload` names.
    ///
    /// # Errors
    ///
    /// When the payload is too large or is not the run event it is named.
    pub(crate) fn ran(self, delivery: Delivery, payload: &[u8]) -> Result<Delivered, SourceError> {
        match (self, delivery) {
            (Self::GitLab, _) => gitlab::delivered(payload),
            (Self::Forgejo, _) => forgejo::delivered(payload),
            (Self::GitHub, Delivery::Run) => github::delivered_run(payload),
            (Self::GitHub, Delivery::CheckSuite) => github::delivered_suite(payload),
            (Self::GitHub, Delivery::Status) => github::delivered_status(payload),
        }
    }

    /// What a run's delivery is read as.
    pub(crate) const fn runs_what(self) -> &'static str {
        match self {
            Self::GitHub => github::RUN,
            Self::GitLab => gitlab::RUN,
            Self::Forgejo => forgejo::RUN,
        }
    }
}

/// The run event a webhook delivery or a CI run's event of `delivery` on
/// `forge` announces, where `choice` announces it.
///
/// # Errors
///
/// When the payload is not the run event it is named, or its run makes no
/// event.
pub(crate) fn delivered_event(
    forge: Forge,
    (delivery, payload): (Delivery, &[u8]),
    choice: &RunChoice,
) -> Result<(String, Option<Event>), SourceError> {
    let delivered = forge.ran(delivery, payload)?;
    let event = delivered
        .run
        .filter(|run| run.chosen(choice))
        .map(|run| run.event(forge.runs_what(), &delivered.repository))
        .transpose()?;
    Ok((delivered.path, event))
}

/// Whether the event `name` on `forge` is of a run.
#[must_use]
pub fn of_a_run(forge: Forge, name: &str) -> bool {
    Delivery::named(forge, name).is_some()
}

/// What a CI run's own event of the name `name`, `payload`, announces for
/// a source of `options` on `forge`: the run's event, from its repository,
/// where it is of a run `runs` chooses; nothing otherwise.
///
/// # Errors
///
/// When the payload is not the run event it is named, or its run makes no
/// event.
pub fn ci_run(
    (forge, options): (Forge, &crate::SourceOptions),
    (name, payload): (&str, &[u8]),
) -> Result<Option<Event>, SourceError> {
    let Some(delivery) = Delivery::named(forge, name) else {
        return Ok(None);
    };
    let (_, event) = delivered_event(forge, (delivery, payload), &options.run_choice())?;
    let Some(event) = event else {
        return Ok(None);
    };
    let delivered = crate::delivered::delivered_run(forge, forge.runs_what(), payload)?;
    let host = crate::forge::host_of(forge, &crate::forge::api_of(forge, options));
    Ok(Some(event.from_origin(crate::delivered::origin(
        forge, &host, delivered,
    ))))
}

/// One listing a poll of runs reads: what it asks, and the validators it
/// keeps under.
struct Listing {
    request: Request,
    kept: Option<Validators>,
    ran: Ran,
}

/// What a poll of a repository's runs or alerts found: their events, the
/// validators of the run listing, and those of each other listing it read.
#[derive(Debug, Default)]
pub(crate) struct Found {
    pub(crate) events: Vec<Event>,
    pub(crate) validators: Option<Validators>,
    pub(crate) listings: BTreeMap<String, Validators>,
    pub(crate) ids: BTreeMap<String, Vec<String>>,
}

/// How a repository's runs are polled.
pub(crate) struct Asking<'a> {
    pub(crate) origin: &'a Origin,
    pub(crate) token: Option<&'a Token>,
    pub(crate) choice: &'a RunChoice,
}

impl Asking<'_> {
    /// The listings a poll reads, each conditional on what `cursor` keeps.
    ///
    /// # Errors
    ///
    /// Where the repository's path names nothing in a URL path.
    fn listings(&self, cursor: Option<&Cursor>) -> Result<Vec<Listing>, Unpolled> {
        let forge = self.origin.forge();
        let page = self.origin.reading().page;
        let mut listings = Vec::new();
        if self.choice.reads(RunFrom::Actions) {
            let kept = cursor
                .and_then(|cursor| cursor.validators(Signal::Run))
                .cloned();
            let path = match forge {
                Forge::GitHub => github::runs_path(self.choice, page),
                Forge::GitLab => gitlab::runs_path(self.choice, page),
                Forge::Forgejo => forgejo::runs_path(self.choice, page),
            };
            listings.push(Listing {
                request: self.conditional(&path, kept.as_ref())?,
                kept,
                ran: Ran::Run,
            });
        }
        if self.choice.reads(RunFrom::Checks) {
            let reference = self.choice.only_branch().unwrap_or(github::HEAD);
            for (path, ran) in github::checks_paths(reference, page) {
                let kept = cursor
                    .and_then(|cursor| cursor.listings.get(ran.name()))
                    .cloned();
                listings.push(Listing {
                    request: self.conditional(&path, kept.as_ref())?,
                    kept,
                    ran,
                });
            }
        }
        Ok(listings)
    }

    /// The request for `path` under the repository, conditional on `kept`.
    fn conditional(&self, path: &str, kept: Option<&Validators>) -> Result<Request, Unpolled> {
        Ok(self.origin.get(path, self.token)?.conditional(
            kept.and_then(|kept| kept.etag.as_deref()),
            kept.and_then(|kept| kept.last_modified.as_deref()),
        ))
    }

    /// The runs of `repository` in `answer` to the listing of `ran`.
    fn listed(
        &self,
        ran: Ran,
        repository: &Repository,
        answer: &Response,
    ) -> Result<Vec<ForgeRun>, SourceError> {
        let forge = self.origin.forge();
        let parse = self.origin.reading().parse;
        let what = listed(forge, ran);
        crate::limited::answered(forge, what, answer)?;
        match (forge, ran) {
            (Forge::GitLab, _) => gitlab::listed(parse, &answer.body),
            (Forge::Forgejo, _) => forgejo::listed(parse, &answer.body),
            (Forge::GitHub, Ran::Run) => github::listed_runs(parse, &answer.body),
            (Forge::GitHub, Ran::CheckSuite) => {
                github::listed_suites(parse, repository, &answer.body)
            }
            (Forge::GitHub, Ran::Status) => {
                let reference = self.choice.only_branch().map(ToOwned::to_owned);
                github::listed_statuses(parse, (repository, reference), &answer.body)
            }
        }
    }

    /// Each listing of the runs since `cursor`, asked through `http`, with
    /// its answer.
    pub(crate) async fn asked(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        cursor: Option<&Cursor>,
    ) -> Result<Asked, Unpolled> {
        let mut answers = Vec::new();
        for listing in self.listings(cursor)? {
            let answer = http
                .send(listing.request)
                .await
                .map_err(Unpolled::unreachable)?;
            answers.push((listing.ran, listing.kept, answer));
        }
        Ok(Asked(answers))
    }

    /// The runs of `repository` the listings `asked` found.
    pub(crate) fn read(&self, asked: &Asked, repository: &Repository) -> Result<Found, Unpolled> {
        let mut runs = Found::default();
        for (ran, kept, answer) in &asked.0 {
            let (ran, answer) = (*ran, answer);
            let kept = if Forge::unchanged(answer) {
                kept.clone().unwrap_or_default()
            } else {
                let what = listed(self.origin.forge(), ran);
                let found = self
                    .listed(ran, repository, answer)
                    .map_err(crate::limited::unread)?;
                for run in found.into_iter().filter(|run| run.chosen(self.choice)) {
                    runs.events
                        .push(run.event(what, repository).map_err(Unpolled::unreadable)?);
                }
                validators(answer)
            };
            match ran {
                Ran::Run => runs.validators = Some(kept),
                Ran::CheckSuite | Ran::Status => {
                    runs.listings.insert(ran.name().to_owned(), kept);
                }
            }
        }
        Ok(runs)
    }
}

/// Each listing a poll of runs asked: what it lists, the validators it
/// was asked under, and its answer.
pub(crate) struct Asked(Vec<(Ran, Option<Validators>, Response)>);

impl Asked {
    /// Whether some listing changed since it was last read.
    pub(crate) fn changed(&self) -> bool {
        self.0
            .iter()
            .any(|(_, _, answer)| !Forge::unchanged(answer))
    }
}

/// What the listing of `ran`'s runs on `forge` is read as.
const fn listed(forge: Forge, ran: Ran) -> &'static str {
    match (forge, ran) {
        (Forge::GitHub, Ran::Run) => github::RUNS,
        (Forge::GitHub, Ran::CheckSuite) => github::SUITES,
        (Forge::GitHub, Ran::Status) => github::STATUSES,
        (Forge::GitLab, _) => gitlab::RUNS,
        (Forge::Forgejo, _) => forgejo::RUNS,
    }
}

/// `polled` with `runs` merged in: their events, and the validators they
/// were read under.
pub(crate) fn merged(mut polled: Polled, runs: Found) -> Polled {
    polled.events.extend(runs.events);
    if let Some(validators) = runs.validators {
        polled.signals.insert(Signal::Run, validators);
    }
    polled.listings.extend(runs.listings);
    polled.ids.extend(runs.ids);
    polled
}

/// `polled`, a poll's answer where it found anything, with `runs`, what the
/// same poll's listings found of the runs or alerts since `cursor`, which
/// `changed` says changed since: nothing where neither changed.
pub(crate) fn merged_into(
    polled: Option<Polled>,
    (changed, runs): (bool, Found),
    cursor: Option<&Cursor>,
) -> Option<Polled> {
    if polled.is_none() && !changed {
        return None;
    }
    let kept = || Polled {
        etag: cursor.and_then(|cursor| cursor.etag.clone()),
        last_modified: cursor.and_then(|cursor| cursor.last_modified.clone()),
        signals: cursor
            .map(|cursor| cursor.signals.clone())
            .unwrap_or_default(),
        packages: cursor
            .map(|cursor| cursor.packages.clone())
            .unwrap_or_default(),
        listings: cursor
            .map(|cursor| cursor.listings.clone())
            .unwrap_or_default(),
        ids: cursor.map(|cursor| cursor.ids.clone()).unwrap_or_default(),
        ..Polled::default()
    };
    let mut polled = merged(polled.unwrap_or_else(kept), runs);
    polled
        .events
        .sort_by_key(|event| std::cmp::Reverse(event.occurred()));
    Some(polled)
}

#[cfg(test)]
mod tests;
