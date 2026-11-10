use std::sync::Arc;

use herald_core::{Cursor, Kind, Polled, Scheme, SecretRef};
use herald_kinds::{Fault, Hooked, Poll, SourceConfig, SourceKind, Unpolled};
use herald_ports::{Boxed, DynHttp, Erased, Http as _, Secret};

use crate::{Forge, Origin, SourceOptions, Token};

/// A repository's releases on a forge: the forge, and the options its
/// table reads into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeSource {
    /// The forge.
    pub forge: Forge,
    /// The options its table reads into.
    pub options: SourceOptions,
}

impl ForgeSource {
    fn checked(forge: Forge, options: SourceOptions) -> Result<Self, Fault> {
        options
            .check(forge)
            .map(|()| Self { forge, options })
            .map_err(Fault::here)
    }
}

/// GitHub, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubKind {}

impl SourceKind for GitHubKind {
    const NAME: &'static str = Forge::GitHub.kind();
    const DESCRIPTION: &'static str = "A GitHub repository's releases.";

    type File = SourceOptions;
    type Options = ForgeSource;

    fn checked(file: SourceOptions) -> Result<ForgeSource, Fault> {
        ForgeSource::checked(Forge::GitHub, file)
    }
}

/// GitLab, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitLabKind {}

impl SourceKind for GitLabKind {
    const NAME: &'static str = Forge::GitLab.kind();
    const DESCRIPTION: &'static str = "A GitLab project's releases.";

    type File = SourceOptions;
    type Options = ForgeSource;

    fn checked(file: SourceOptions) -> Result<ForgeSource, Fault> {
        ForgeSource::checked(Forge::GitLab, file)
    }
}

/// Forgejo or Gitea, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgejoKind {}

impl SourceKind for ForgejoKind {
    const NAME: &'static str = Forge::Forgejo.kind();
    const DESCRIPTION: &'static str = "A Forgejo or Gitea repository's releases.";

    type File = SourceOptions;
    type Options = ForgeSource;

    fn checked(file: SourceOptions) -> Result<ForgeSource, Fault> {
        ForgeSource::checked(Forge::Forgejo, file)
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
        &[Kind::Release]
    }

    fn backfill(&self) -> usize {
        self.options.backfill
    }

    fn scheme(&self) -> Option<Scheme> {
        Some(self.forge.scheme())
    }

    fn hooked(&self, headers: &[(&str, &str)], payload: &[u8]) -> Result<Hooked, String> {
        self.forge
            .hooked(headers, payload, &self.options.repository)
            .map_err(|error| error.to_string())
    }

    fn poller(&self, token: Option<Secret>) -> Arc<dyn Poll> {
        Arc::new(Releases {
            origin: Origin::new(self.forge, &self.options),
            token: token.map(Token::Named),
        })
    }
}

/// A repository's releases, polled with the token the config names.
#[derive(Debug)]
struct Releases {
    origin: Origin,
    token: Option<Token>,
}

impl Poll for Releases {
    fn api(&self) -> &str {
        self.origin.api()
    }

    fn remembered(&self) -> usize {
        self.origin.reading().remembered
    }

    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        cursor: Option<&'a Cursor>,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(async move {
            let http = Erased(http);
            let token = self.token.as_ref();
            let answer = http
                .send(self.origin.releases_request(cursor, token))
                .await
                .map_err(Unpolled::unreachable)?;
            if Forge::unchanged(&answer) {
                return Ok(None);
            }
            let described = http
                .send(self.origin.repository_request(token))
                .await
                .map_err(Unpolled::unreachable)?;
            let forge = self.origin.forge();
            let repository = forge.repository(&described).map_err(Unpolled::unreadable)?;
            forge
                .releases(&repository, &answer, self.origin.reading().parse)
                .map(Some)
                .map_err(Unpolled::unreadable)
        })
    }
}

#[cfg(test)]
mod tests;
