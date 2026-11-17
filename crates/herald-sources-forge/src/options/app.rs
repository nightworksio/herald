//! The keys that have a GitHub source read as a GitHub App (ADR-0041).

use herald_core::kind_name;
use herald_kinds::{Installation, Installed};

use super::SourceOptions;
use crate::Forge;
use crate::forge::api_of;

impl SourceOptions {
    /// Whether the source reads with a token: one the config names, or one
    /// its GitHub App mints.
    #[must_use]
    pub const fn reads_with_token(&self) -> bool {
        self.token.is_some() || self.app.is_some()
    }

    /// Whether the keys naming a GitHub App are sound on `forge`: given on
    /// GitHub only, in place of `token`, `installation` only beside `app`.
    pub(super) fn app_check(&self, forge: Forge) -> Result<(), String> {
        match (&self.app, self.installation) {
            (Some(_), _) if forge != Forge::GitHub => {
                Err("app is a GitHub source's; remove it, or give token".to_owned())
            }
            (Some(_), _) if self.token.is_some() => Err(
                "a source reads with its App's tokens or with token, not both; remove one".to_owned(),
            ),
            (Some(app), _) if !kind_name(app) => Err(format!(
                "\"{app}\" is not an App's name; give the name of its [github_apps.<name>] table"
            )),
            (None, Some(_)) => Err(
                "installation names the App's installation a token is minted for; give app, or remove installation".to_owned(),
            ),
            _ => Ok(()),
        }
    }

    /// The GitHub App the source on `forge` reads as, where it names one,
    /// and the installation its tokens are minted for: the one
    /// `installation` names, or else the owner's or the repository's.
    #[must_use]
    pub fn installed(&self, forge: Forge) -> Option<Installed<'_>> {
        let installation = match (self.installation, &self.owner, &self.repository) {
            (Some(id), _, _) => Installation::Fixed(id.get()),
            (None, Some(owner), _) => Installation::Owner(owner),
            (None, None, Some(repository)) => Installation::Repository(repository),
            (None, None, None) => return None,
        };
        Some(Installed {
            app: self.app.as_deref()?,
            api: api_of(forge, self),
            installation,
        })
    }
}
