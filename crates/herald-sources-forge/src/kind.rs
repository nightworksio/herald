use std::sync::Arc;

use herald_core::{Kind, Polled, Scheme, SecretRef, Selection};
use herald_kinds::{Fault, Hooked, Polling, SourceConfig, SourceKind, Unpolled};
use herald_ports::Secret;

use crate::owner::Owned;
use crate::polled::Releases;
use crate::{Forge, Members, Origin, SourceOptions, Token};

/// A repository's releases and tags on a forge: the forge, and the options
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

/// GitHub, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubKind {}

impl SourceKind for GitHubKind {
    const NAME: &str = Forge::GitHub.kind();
    const DESCRIPTION: &str = "A GitHub repository's releases and tags.";

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
    const DESCRIPTION: &str = "A GitLab project's releases and tags.";

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
    const DESCRIPTION: &str = "A Forgejo or Gitea repository's releases and tags.";

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
        &[Kind::RELEASE]
    }

    fn notes(&self) -> Vec<&'static str> {
        if self.options.webhook.is_none() {
            return Vec::new();
        }
        let owned = self.options.owner.as_ref().map(|_| OWNER_HOOKED);
        self.forge.unsigned().into_iter().chain(owned).collect()
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

    fn scheme(&self) -> Option<Scheme> {
        Some(self.forge.scheme())
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
                repository,
                &self.options.signals_of(named(repository)),
            )
        };
        hooked.map_err(|error| error.to_string())
    }

    fn poller(&self, token: Option<Secret>) -> Polling {
        let token = token.map(Token::Named);
        match (&self.options.owner, &self.options.repository) {
            (Some(owner), _) => Polling::Many(Arc::new(Owned::new(
                self.forge,
                (owner, self.names.as_deref()),
                &self.options,
                token,
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

#[cfg(test)]
mod tests;
