use std::sync::Arc;

use herald_core::{Event, Kind, Polled, Scheme, SecretRef, Selection, Signal};
use herald_kinds::{
    Credential, Fault, Hooked, Installed, Polling, Registering, SourceConfig, SourceKind, Unpolled,
};

use herald_wire::SourceError;

use crate::alerts;
use crate::forge::{UNSELECTED, api_of, host_of};
use crate::owner::Owned;
use crate::packages::{PackageList, Packaging};
use crate::polled::Releases;
use crate::runs::{Delivery, delivered_event};
use crate::{
    Discovery, Forge, Members, MilestoneAction, Origin, RunFrom, SourceOptions, Token, Transport,
};

/// A repository's releases, tags and packages on a forge: the forge, and the options
/// its table reads into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeSource {
    /// The forge.
    pub forge: Forge,
    /// The options its table reads into.
    pub options: SourceOptions,
    /// The names its member file beside the config lists, once read.
    pub names: Option<Vec<String>>,
}

impl ForgeSource {
    fn checked(forge: Forge, options: SourceOptions) -> Result<Self, Fault> {
        options
            .check(forge)
            .map(|()| Self {
                forge,
                options,
                names: None,
            })
            .map_err(Fault::here)
    }

    /// The source with the member file beside the config it names read
    /// through `read`.
    fn read(mut self, read: &dyn Fn(&str) -> Result<String, String>) -> Result<Self, Fault> {
        if let Some(Members::File { file }) = &self.options.members {
            let names = read(file)
                .and_then(|text| {
                    crate::members::names(file, &text, &self.options.tables, self.options.key())
                })
                .map_err(|reason| Fault::at("members.file", reason))?;
            self.names = Some(names);
        }
        Ok(self)
    }
}

/// What the doctor says of an owner source that takes webhooks.
const OWNER_HOOKED: &str = "a webhook's release is announced only where the owner's last membership record holds its repository, so poll the source as well; a repository its polls have not listed announces nothing";

/// What doctor says of an owner source announcing runs through the owner's
/// events or feed.
const RUNS_UNFOLLOWED: &str = "the owner's events name no runs, so a source announcing runs polls every member each round, conditionally; a webhook or a GitHub App delivers them as they finish";
/// What doctor says of an owner source announcing alerts through the
/// owner's events or feed.
const ALERTS_UNFOLLOWED: &str = "the owner's events name no alerts, so a source announcing alerts polls every member each round; a webhook or a GitHub App delivers them as they change";
/// What doctor says of a source announcing alerts with no webhook.
const ALERTS_POLLED: &str = "a poll announces the first sighting and the closing of the alerts it lists, the newest of each tool's; a reopening, and the closing of an older alert, comes only by webhook";
/// What doctor says of a GitHub source announcing alerts.
const GITHUB_ALERTS: &str = "GitHub lists a tool's alerts where the repository has it enabled, to a token with security_events or a GitHub App that may read code scanning, Dependabot and secret scanning alerts; narrow alerts.tools to the tools it has";
/// What doctor says of a GitLab source announcing alerts.
const GITLAB_ALERTS: &str = "GitLab raises vulnerabilities only on GitLab Ultimate, to a token whose user may read them; a vulnerability set back to detected reads as its first sighting, so a reopening is not announced";
/// What doctor says of an owner source announcing pull requests through
/// the owner's events or feed.
const PULLS_UNFOLLOWED: &str = "an owner source announcing pull requests polls every member each round; a webhook or a GitHub App delivers them as they change";
/// What doctor says of a source announcing pull requests with no webhook.
const PULLS_POLLED: &str = "a poll announces the opening and the merge or close of the pull requests it lists, the newest; a pull request readied for review or reopened comes only by webhook";
/// What doctor says of an owner source announcing issues through the
/// owner's events or feed.
const ISSUES_UNFOLLOWED: &str = "an owner source announcing issues polls every member each round; a webhook or a GitHub App delivers them as they change";
/// What doctor says of a source announcing issues with no webhook.
const ISSUES_POLLED: &str = "a poll announces the opening and the closing of the issues it lists, the newest; a reopened issue comes only by webhook";
/// What doctor says of a source announcing milestones: what a poll of them
/// costs.
const MILESTONES_COST: &str = "a poll asks each repository's open milestones once a round, conditionally, a page at a time up to milestones.most, and reads once each milestone that left the listing";
/// What doctor says of an owner source announcing milestones through the
/// owner's events or feed.
const MILESTONES_UNFOLLOWED: &str = "an owner source announcing milestones polls every member each round; a webhook delivers them as they change";
/// What doctor says of a source announcing milestones with no webhook.
const MILESTONES_POLLED: &str = "a poll finds each milestone opened, closed or reopened within one round; a deleted milestone is withdrawn only by webhook";
/// What doctor says of a source announcing milestones overdue.
const MILESTONES_OVERDUE: &str = "no forge sends a milestone overdue, so only a poll finds one, by its own clock, on its first round after the due day ends in UTC, and again where a moved due day passes; a source only delivered webhooks and never polled never announces it";
/// What doctor says of a Forgejo source announcing milestones.
const FORGEJO_MILESTONES: &str = "Forgejo and Gitea send no milestone webhook (issue_milestone is an issue's change), so milestones are found only by polling";
/// What doctor says of a GitLab source announcing milestones through a
/// webhook.
const GITLAB_MILESTONE_HOOK: &str = "GitLab sends its Milestone Hook from GitLab 18.2, so poll an older self-managed instance; the hook counts no issues, so a milestone it delivers names no counts";
/// What doctor says of a GitLab source counting milestones' issues.
const GITLAB_COUNTS: &str = "GitLab counts a milestone's issues only, and each milestone a poll announces costs one request more for them; milestones.counts = false saves it";
/// What doctor says of a GitHub source counting milestones' issues.
const GITHUB_COUNTS: &str =
    "GitHub counts a milestone's issues and pull requests together, at no cost";
/// What doctor says of a GitLab source announcing pull requests or issues
/// through a webhook.
const GITLAB_AUTHORS: &str = "GitLab's merge request and issue webhooks name only who acted, so an author is known from the opening alone, and pulls.authors and issues.authors take no other webhook event";
/// What doctor says of a source reading checks by polling.
const CHECKS_POLLED: &str = "a poll reads the check suites and statuses of one commit, the newest of the one branch runs.branches names or of the default branch; a webhook delivers every commit's";

/// What doctor says of a source discovering by push that names no webhook.
const UNPUSHED: &str = "discovery \"push\" learns of the owner's repositories joining and leaving through its webhook, which the source names none of, so it lists the owner only where it holds no listing or its selection changed";
/// What doctor says of a GitLab group taking its own webhooks.
const GITLAB_GROUPED: &str = "GitLab sends a group's own webhooks only on Premium or Ultimate; on Free, set member_hooks so each project sends its own, or let herald poll the group";
/// What doctor says of a GitLab source discovering by push.
const GITLAB_UNPUSHED: &str = "herald reads no GitLab event of a project joining or leaving a group, so discovery \"push\" lists the group only where it holds no listing or its selection changed";

/// GitHub, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubKind {}

impl SourceKind for GitHubKind {
    const NAME: &str = Forge::GitHub.kind();
    const DESCRIPTION: &str = "A GitHub repository's releases, tags and packages.";

    type File = SourceOptions;
    type Options = ForgeSource;

    fn checked(file: SourceOptions) -> Result<ForgeSource, Fault> {
        ForgeSource::checked(Forge::GitHub, file)
    }

    fn read(
        options: ForgeSource,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<ForgeSource, Fault> {
        options.read(read)
    }
}

/// GitLab, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitLabKind {}

impl SourceKind for GitLabKind {
    const NAME: &str = Forge::GitLab.kind();
    const DESCRIPTION: &str = "A GitLab project's releases, tags and packages.";

    type File = SourceOptions;
    type Options = ForgeSource;

    fn checked(file: SourceOptions) -> Result<ForgeSource, Fault> {
        ForgeSource::checked(Forge::GitLab, file)
    }

    fn read(
        options: ForgeSource,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<ForgeSource, Fault> {
        options.read(read)
    }
}

/// Forgejo or Gitea, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgejoKind {}

impl SourceKind for ForgejoKind {
    const NAME: &str = Forge::Forgejo.kind();
    const DESCRIPTION: &str = "A Forgejo or Gitea repository's releases, tags and packages.";

    type File = SourceOptions;
    type Options = ForgeSource;

    fn checked(file: SourceOptions) -> Result<ForgeSource, Fault> {
        ForgeSource::checked(Forge::Forgejo, file)
    }

    fn read(
        options: ForgeSource,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<ForgeSource, Fault> {
        options.read(read)
    }
}

impl SourceConfig for ForgeSource {
    fn kind(&self) -> &'static str {
        self.forge.kind()
    }

    fn token(&self) -> Option<&SecretRef> {
        self.options.token.as_ref()
    }

    fn webhook(&self) -> Option<&SecretRef> {
        self.options.webhook.as_ref()
    }

    fn kinds(&self) -> Vec<Kind> {
        let mut kinds: Vec<Kind> = Vec::new();
        for kind in Signal::ALL
            .into_iter()
            .filter(|signal| self.options.may_take(*signal))
            .map(Signal::kind)
        {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        kinds
    }

    fn notes(&self) -> Vec<&'static str> {
        let pushed = self.pushed();
        let grouped = self.forge == Forge::GitLab
            && self.options.owner.is_some()
            && self.options.webhook.is_some()
            && self.options.member_hooks() == crate::MemberHooks::Off;
        let followed = self.options.owner.is_some()
            && matches!(
                self.options.transport(),
                Transport::Events | Transport::Feed
            );
        let ran = self.options.may_take(Signal::Run);
        let alerted = self.options.may_take(Signal::Alert);
        let pulled = self.options.may_take(Signal::PullRequest);
        let issued = self.options.may_take(Signal::Issue);
        let milestoned = self.options.may_take(Signal::Milestone);
        let chosen = self.options.milestone_choice();
        let counted = milestoned && chosen.counts;
        let overdue = milestoned && chosen.actions.contains(&MilestoneAction::Overdue);
        let hooked = self.options.webhook.is_some();
        let checked = self.options.run_choice().reads(RunFrom::Checks);
        let unheard = [
            (ran && followed, RUNS_UNFOLLOWED),
            (alerted && followed, ALERTS_UNFOLLOWED),
            (alerted && self.options.webhook.is_none(), ALERTS_POLLED),
            (alerted && self.forge == Forge::GitHub, GITHUB_ALERTS),
            (alerted && self.forge == Forge::GitLab, GITLAB_ALERTS),
            (pulled && followed, PULLS_UNFOLLOWED),
            (pulled && !hooked, PULLS_POLLED),
            (issued && followed, ISSUES_UNFOLLOWED),
            (issued && !hooked, ISSUES_POLLED),
            (milestoned, MILESTONES_COST),
            (milestoned && followed, MILESTONES_UNFOLLOWED),
            (milestoned && !hooked, MILESTONES_POLLED),
            (overdue, MILESTONES_OVERDUE),
            (
                milestoned && self.forge == Forge::Forgejo,
                FORGEJO_MILESTONES,
            ),
            (
                milestoned && hooked && self.forge == Forge::GitLab,
                GITLAB_MILESTONE_HOOK,
            ),
            (counted && self.forge == Forge::GitLab, GITLAB_COUNTS),
            (counted && self.forge == Forge::GitHub, GITHUB_COUNTS),
            (
                (pulled || issued) && hooked && self.forge == Forge::GitLab,
                GITLAB_AUTHORS,
            ),
            (checked && self.options.webhook.is_none(), CHECKS_POLLED),
            (pushed && self.options.webhook.is_none(), UNPUSHED),
            (pushed && self.forge == Forge::GitLab, GITLAB_UNPUSHED),
            (grouped, GITLAB_GROUPED),
        ]
        .into_iter()
        .filter_map(|(heard, note)| heard.then_some(note));
        if self.options.webhook.is_none() {
            return unheard.collect();
        }
        let owned = self.options.owner.as_ref().map(|_| OWNER_HOOKED);
        self.forge
            .unsigned()
            .into_iter()
            .chain(owned)
            .chain(unheard)
            .collect()
    }

    fn backfill(&self) -> usize {
        self.options.backfill
    }

    fn unlisted(&self) -> bool {
        self.options.owner.is_some()
            && self
                .options
                .selection()
                .is_ok_and(|selection| selection.takes_unlisted())
    }

    fn holds(&self) -> bool {
        self.options.prefer() == herald_core::Prefer::Release
            && self.options.may_take(herald_core::Signal::Release)
            && self.options.may_take(herald_core::Signal::Tag)
    }

    fn signals(&self) -> Vec<herald_core::Signal> {
        herald_core::Signal::ALL
            .into_iter()
            .filter(|signal| self.options.may_take(*signal))
            .collect()
    }

    fn tag_window(&self) -> Option<jiff::SignedDuration> {
        let repository = self.options.repository.as_deref()?;
        self.options
            .held_for(&self.options.signals_of(named(repository)))
    }

    fn scheme(&self) -> Option<Scheme> {
        Some(self.forge.scheme())
    }

    fn origins(&self) -> bool {
        true
    }

    fn pushed(&self) -> bool {
        self.options.discovery() == Discovery::Push
    }

    fn installed(&self) -> Option<Installed<'_>> {
        self.options.installed(self.forge)
    }

    fn registering(&self) -> Registering {
        self.options.registering()
    }

    fn routed(&self) -> Vec<(String, String)> {
        self.options.routed()
    }

    fn standing(&self) -> Option<herald_kinds::Standing> {
        self.options.repository.as_deref().map(|repository| {
            herald_kinds::Standing::new(&api_of(self.forge, &self.options), repository, false)
        })
    }

    fn projects(&self) -> Vec<String> {
        self.options
            .repository
            .as_deref()
            .map(|repository| named(repository).to_owned())
            .into_iter()
            .collect()
    }

    fn hooked(&self, headers: &[(&str, &str)], payload: &[u8]) -> Result<Hooked, String> {
        if let Some(delivery) = self.forge.run_delivery(headers) {
            return self
                .ran(delivery, payload)
                .map_err(|error| error.to_string());
        }
        if let Some(delivery) = self.forge.alert_delivery(headers) {
            return self
                .alerted(delivery, payload)
                .map_err(|error| error.to_string());
        }
        if self.forge.pull_delivery(headers) {
            return self.pulled(payload).map_err(|error| error.to_string());
        }
        if self.forge.milestone_delivery(headers) {
            return self.milestoned(payload).map_err(|error| error.to_string());
        }
        if let Some(name) = self.forge.issue_event(headers) {
            return self
                .issued((name, payload))
                .map_err(|error| error.to_string());
        }
        let hooked = if let Some(owner) = &self.options.owner {
            self.forge.hooked_owned(
                (headers, payload),
                (owner, self.options.subgroups()),
                (
                    &Selection {
                        names: self.names.clone(),
                        ..self.options.selection().unwrap_or_default()
                    },
                    &|name| self.options.signals_of(name),
                ),
            )
        } else {
            let repository = self.options.repository.as_deref().unwrap_or_default();
            self.forge.hooked(
                (headers, payload),
                (
                    repository,
                    &host_of(self.forge, &api_of(self.forge, &self.options)),
                ),
                &self.options.signals_of(named(repository)),
            )
        };
        hooked.map_err(|error| error.to_string())
    }

    fn signing(&self) -> Result<herald_kinds::SignIn, &'static str> {
        self.options.signing(self.forge)
    }

    fn write(&self) -> Option<herald_kinds::WriteCredential<'_>> {
        self.options.writing(self.forge)
    }

    fn rerunner(
        &self,
        read: Option<Credential>,
    ) -> Result<Arc<dyn herald_kinds::Rerunner>, &'static str> {
        if self.forge == Forge::Forgejo {
            return Err(crate::options::FORGEJO_UNWRITTEN);
        }
        let credential = self
            .write()
            .ok_or(herald_kinds::UNWRITTEN)?
            .name()
            .to_owned();
        Ok(Arc::new(crate::rerunner::ForgeRerunner {
            forge: self.forge,
            api: crate::forge::api_of(self.forge, &self.options),
            parse: self.options.reading().parse,
            read: read.map(Token::of),
            credential,
            signing: self.options.signing(self.forge),
        }))
    }

    fn queue(&self, token: Option<Credential>) -> Option<Arc<dyn herald_kinds::Queue>> {
        let api = api_of(self.forge, &self.options);
        if let Some(owner) = &self.options.owner {
            let owned = Owned::new(
                self.forge,
                (owner, self.names.as_deref()),
                &self.options,
                (None, 0),
            );
            return Some(Arc::new(crate::queue::Scoped::new(
                (self.forge, owner),
                &self.options,
                token.map(Token::of),
                (herald_wire::github::graphql(&api), owned),
            )));
        }
        let repository = self.options.repository.as_deref()?;
        Some(Arc::new(crate::queue::Queued {
            origin: Origin::new(self.forge, &self.options, repository),
            token: token.map(Token::of),
            project: named(repository).to_owned(),
            graphql: herald_wire::github::graphql(&api),
        }))
    }

    fn folded(
        &self,
        headers: &[(&str, &str)],
        payload: &[u8],
    ) -> Result<Option<herald_core::Folding>, String> {
        let Some(name) = self.forge.event_named(headers) else {
            return Ok(None);
        };
        let of = match (&self.options.owner, self.options.repository.as_deref()) {
            (Some(owner), _) => crate::folding::Of::Owner(owner, self.options.subgroups()),
            (None, Some(repository)) => crate::folding::Of::Repository(repository),
            (None, None) => return Ok(None),
        };
        crate::folding::folded(self.forge, (name, payload), of).map_err(|error| error.to_string())
    }

    fn areas(&self, repository: &str) -> Option<&herald_core::Zoning> {
        self.options.areas_of(named(repository))
    }

    fn codeowning(&self, repository: &str) -> Option<herald_kinds::Codeowning> {
        self.options.codeowning_of(self.forge, named(repository))
    }

    fn codeowned(&self) -> bool {
        self.options.codeowned()
    }

    fn codeowners_notes(&self) -> Vec<String> {
        self.options.codeowners_notes(self.forge)
    }

    fn area_names(&self) -> std::collections::BTreeSet<String> {
        self.options.area_names()
    }

    fn forge_teams(&self) -> bool {
        self.options.forge_teams()
    }

    fn teams_note(&self) -> Option<String> {
        self.forge_teams()
            .then(|| crate::queue::teams::note(self.forge))
    }

    fn queue_cost(&self, most: herald_kinds::Most) -> Option<herald_kinds::QueueCost> {
        let tokened = self.token().is_some() || self.installed().is_some();
        if self.options.owner.is_some() {
            return Some(crate::queue::scope_cost(self.forge, tokened, most));
        }
        self.options.repository.as_ref()?;
        Some(crate::queue::cost(self.forge, tokened, most))
    }

    fn poller(&self, token: Option<Credential>, edits: bool) -> Polling {
        let token = token.map(Token::of);
        let watched = self.options.watched(edits);
        match (&self.options.owner, &self.options.repository) {
            (Some(owner), _) => Polling::Many(Arc::new(Owned::new(
                self.forge,
                (owner, self.names.as_deref()),
                &self.options,
                (token, watched),
            ))),
            (None, repository) => {
                let repository = repository.as_deref().unwrap_or_default();
                let signals = self.options.signals_of(named(repository));
                Polling::One(Arc::new(Releases {
                    origin: Origin::new(self.forge, &self.options, repository),
                    token,
                    project: None,
                    held: self.options.held_for(&signals),
                    signals,
                    backfill: self.options.backfill,
                    watched,
                    packaging: Packaging::of(self.forge, &self.options),
                    list: PackageList::Own,
                    visibility: None,
                    runs: self.options.run_choice(),
                    alerts: self.options.alert_choice(self.forge),
                    pulls: self.options.pull_choice(),
                    issues: self.options.issue_choice(),
                    milestones: self.options.milestone_choice(),
                }))
            }
        }
    }
}

/// Why a run delivery announces nothing where the source announces no runs
/// of its repository.
const UNRAN: &str = "the source announces no runs of the repository";
/// Why a run delivery announces nothing where the source does not announce
/// that run.
const UNCHOSEN: &str = "the run is not one the source's runs announce";
/// Why an alert delivery announces nothing where the source announces no
/// alerts of its repository.
const UNALERTED: &str = "the source announces no alerts of the repository";
/// Why an alert delivery announces nothing where the source does not
/// announce that alert.
const UNCHOSEN_ALERT: &str = "the alert is not one the source's alerts announce";

/// Why a pull request delivery announces nothing where the source
/// announces no pull requests of its repository.
const UNPULLED: &str = "the source announces no pull requests of the repository";
/// Why a pull request delivery announces nothing where the source does not
/// announce that pull request.
const UNCHOSEN_PULL: &str = "the pull request is not one the source's pulls announce";

/// Why an issue delivery announces nothing where the source announces no
/// issues of its repository.
const UNISSUED: &str = "the source announces no issues of the repository";
/// Why an issue delivery announces nothing where the source does not
/// announce that issue.
const UNCHOSEN_ISSUE: &str = "the issue is not one the source's issues announce";

/// Why a milestone delivery announces nothing where the source announces
/// no milestones of its repository.
const UNMILESTONED: &str = "the source announces no milestones of the repository";
/// Why a milestone delivery announces nothing where the source does not
/// announce that milestone.
const UNCHOSEN_MILESTONE: &str = "the milestone is not one the source's milestones announce";
/// Why a milestone delivery announces nothing where it names no project.
const UNPROJECTED_MILESTONE: &str = "a GitLab group's own milestone webhook names no project; milestones.group reads the group's milestones by polling";

/// What a delivery of a signal names: what it is read as, the signal, the
/// repository's full path, id and description, and the event it makes
/// where the source's choice announces it.
struct Signalled {
    what: &'static str,
    signal: Signal,
    path: String,
    id: String,
    delivered: herald_core::Delivered,
    event: Option<Event>,
}

impl ForgeSource {
    /// What the run delivery `delivery`, `payload`, asks the source to
    /// announce.
    fn ran(&self, delivery: Delivery, payload: &[u8]) -> Result<Hooked, SourceError> {
        let what = self.forge.runs_what();
        let (path, event) =
            delivered_event(self.forge, (delivery, payload), &self.options.run_choice())?;
        let (id, delivered) = crate::delivered::delivered_run(self.forge, what, payload)?;
        let signalled = Signalled {
            what,
            signal: Signal::Run,
            path,
            id,
            delivered,
            event,
        };
        self.signalled(signalled, (UNRAN, UNCHOSEN))
    }

    /// What the alert delivery `delivery`, `payload`, asks the source to
    /// announce.
    fn alerted(&self, delivery: alerts::Delivery, payload: &[u8]) -> Result<Hooked, SourceError> {
        let what = delivery.what();
        let api = api_of(self.forge, &self.options);
        let choice = self.options.alert_choice(self.forge);
        let found = alerts::delivered((delivery, &api), payload, &choice)?;
        let (id, delivered) = match delivery {
            alerts::Delivery::GitHub(_) => crate::delivered::delivered(self.forge, what, payload)?,
            alerts::Delivery::GitLab => (
                found.repository.id.to_string(),
                herald_core::Delivered {
                    name: found.repository.name.clone(),
                    path: found.path.clone(),
                    ..herald_core::Delivered::default()
                },
            ),
        };
        let signalled = Signalled {
            what,
            signal: Signal::Alert,
            path: found.path,
            id,
            delivered,
            event: found.event,
        };
        self.signalled(signalled, (UNALERTED, UNCHOSEN_ALERT))
    }

    /// What the pull request delivery `payload` asks the source to
    /// announce.
    fn pulled(&self, payload: &[u8]) -> Result<Hooked, SourceError> {
        let what = crate::pulls::hooked_what(self.forge);
        let found = crate::pulls::delivered(self.forge, payload, &self.options.pull_choice())?;
        let (id, delivered) = crate::delivered::delivered(self.forge, what, payload)?;
        let signalled = Signalled {
            what,
            signal: Signal::PullRequest,
            path: found.path,
            id,
            delivered,
            event: found.event,
        };
        self.signalled(signalled, (UNPULLED, UNCHOSEN_PULL))
    }

    /// What the issue delivery `payload`, of the event named `name`, asks
    /// the source to announce.
    fn issued(&self, (name, payload): (&str, &[u8])) -> Result<Hooked, SourceError> {
        let what = crate::issues::hooked_what(self.forge);
        let found =
            crate::issues::delivered(self.forge, (name, payload), &self.options.issue_choice())?;
        let (id, delivered) = crate::delivered::delivered(self.forge, what, payload)?;
        let signalled = Signalled {
            what,
            signal: Signal::Issue,
            path: found.path,
            id,
            delivered,
            event: found.event,
        };
        self.signalled(signalled, (UNISSUED, UNCHOSEN_ISSUE))
    }

    /// What the milestone delivery `payload` asks the source to announce:
    /// nothing where it names no project, as a GitLab group's own webhook
    /// does.
    fn milestoned(&self, payload: &[u8]) -> Result<Hooked, SourceError> {
        let what = crate::milestones::hooked_what(self.forge);
        let choice = self.options.milestone_choice();
        let Some(found) = crate::milestones::delivered(self.forge, payload, &choice)? else {
            return Ok(Hooked::Nothing(UNPROJECTED_MILESTONE));
        };
        let (id, delivered) = crate::delivered::delivered(self.forge, what, payload)?;
        let signalled = Signalled {
            what,
            signal: Signal::Milestone,
            path: found.path,
            id,
            delivered,
            event: found.event,
        };
        self.signalled(signalled, (UNMILESTONED, UNCHOSEN_MILESTONE))
    }

    /// What the delivery `signalled` asks the source to announce: its event
    /// where the source announces its signal of its repository and chooses
    /// it, as of an owner's member where the source names an owner, and
    /// else nothing, for `unsignalled` or `unchosen`.
    fn signalled(
        &self,
        signalled: Signalled,
        (unsignalled, unchosen): (&'static str, &'static str),
    ) -> Result<Hooked, SourceError> {
        let Signalled {
            what,
            signal,
            path,
            id,
            delivered,
            event,
        } = signalled;
        let unheld = |reason: String| SourceError::Unreadable { what, reason };
        let (owned, admitted) = if let Some(owner) = &self.options.owner {
            let holder = path.rsplit_once('/').map_or("", |(holder, _)| holder);
            if !self.forge.holds((owner, self.options.subgroups()), holder) {
                return Err(unheld(format!(
                    "it is not of a repository {owner}, the owner its source names, holds"
                )));
            }
            let selection = Selection {
                names: self.names.clone(),
                ..self.options.selection().unwrap_or_default()
            };
            (true, selection.admits(&delivered))
        } else {
            let repository = self.options.repository.as_deref().unwrap_or_default();
            if !path.eq_ignore_ascii_case(repository) {
                return Err(unheld(format!(
                    "it is not of {repository}, the repository its source names"
                )));
            }
            (false, true)
        };
        if !admitted {
            return Ok(Hooked::Nothing(UNSELECTED));
        }
        if !self.options.signals_of(named(&path)).takes(signal) {
            return Ok(Hooked::Nothing(unsignalled));
        }
        let Some(event) = event else {
            return Ok(Hooked::Nothing(unchosen));
        };
        if owned {
            return Ok(Hooked::Member {
                visibility: delivered.visibility,
                event,
                id,
            });
        }
        let host = host_of(self.forge, &api_of(self.forge, &self.options));
        let origin = crate::delivered::origin(self.forge, &host, (id, delivered));
        Ok(Hooked::Event(event.from_origin(origin)))
    }
}

/// The name of the repository at `path`, without its owner.
pub(crate) fn named(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `polled` with each event announced as of the project a template named,
/// where one did, or why it named none.
pub(crate) fn projected(
    polled: Polled,
    project: Option<Result<String, String>>,
) -> Result<Polled, Unpolled> {
    match project {
        None => Ok(polled),
        Some(Err(reason)) => Err(Unpolled::Unreadable(reason)),
        Some(Ok(project)) => Ok(Polled {
            events: polled
                .events
                .into_iter()
                .map(|event| event.of_project(&project))
                .collect::<Result<_, _>>()
                .map_err(Unpolled::unreadable)?,
            ..polled
        }),
    }
}

/// `polled`, each of its events from `origin`.
pub(crate) fn originated(polled: Polled, origin: &herald_core::Origin) -> Polled {
    Polled {
        events: polled
            .events
            .into_iter()
            .map(|event| event.from_origin(origin.clone()))
            .collect(),
        ..polled
    }
}

#[cfg(test)]
mod tests;
