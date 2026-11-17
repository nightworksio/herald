use std::sync::Arc;

use herald_core::{Kind, Polled, Scheme, SecretRef, Selection, Signal};
use herald_kinds::{
    Credential, Fault, Hooked, Installed, Polling, Registering, SourceConfig, SourceKind, Unpolled,
};

use crate::forge::{api_of, host_of};
use crate::owner::Owned;
use crate::packages::{PackageList, Packaging};
use crate::polled::Releases;
use crate::{Discovery, Forge, Members, Origin, SourceOptions, Token};

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

    fn kinds(&self) -> &'static [Kind] {
        let released = self.options.may_take(Signal::Release) || self.options.may_take(Signal::Tag);
        match (released, self.options.may_take(Signal::Package)) {
            (true, true) => &[Kind::RELEASE, Kind::PACKAGE_VERSION],
            (false, true) => &[Kind::PACKAGE_VERSION],
            (_, false) => &[Kind::RELEASE],
        }
    }

    fn notes(&self) -> Vec<&'static str> {
        let pushed = self.pushed();
        let grouped = self.forge == Forge::GitLab
            && self.options.owner.is_some()
            && self.options.webhook.is_some()
            && self.options.member_hooks() == crate::MemberHooks::Off;
        let unheard = [
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
                }))
            }
        }
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
