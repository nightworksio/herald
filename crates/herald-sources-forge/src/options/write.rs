//! The keys that let an operator re-run a planned job: the source's write
//! credential and how the operator signs in (ADR-0068).

pub use herald_core::merging::Write;
use herald_core::{Permission, web_url};
use herald_kinds::{Installation, Installed, SignIn as Signing, WriteCredential};

use super::SourceOptions;
use crate::Forge;
use crate::forge::api_of;

/// The scopes a GitHub sign-in asks for where `scopes` is absent: none, so
/// an OAuth application's token reads public repositories alone and a
/// GitHub App's holds only its own read permissions.
pub const GITHUB_SCOPES: [&str; 0] = [];

/// The scopes a GitLab sign-in asks for where `scopes` is absent: reading
/// the API, the narrowest that reads a member's role on a project.
pub const GITLAB_SCOPES: [&str; 1] = ["read_api"];

/// Every scope a GitHub sign-in may ask for or hold: each reads alone.
pub const GITHUB_READ_SCOPES: [&str; 2] = ["read:user", "read:org"];

/// Every scope a GitLab sign-in may ask for or hold: each reads alone.
pub const GITLAB_READ_SCOPES: [&str; 5] = ["read_api", "read_user", "openid", "profile", "email"];

/// Every scope an operator's token on `forge` may hold: none that writes.
#[must_use]
pub const fn read_only(forge: Forge) -> &'static [&'static str] {
    match forge {
        Forge::GitLab => &GITLAB_READ_SCOPES,
        Forge::GitHub | Forge::Forgejo => &GITHUB_READ_SCOPES,
    }
}

/// The first of `scopes` an operator's token on `forge` may not hold, as
/// it can do more than read.
pub fn unread<'a>(forge: Forge, scopes: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    scopes
        .into_iter()
        .find(|scope| !read_only(forge).contains(scope))
}

/// Why a write credential that is an App is refused off GitHub.
pub const GITHUB_APPS: &str = "write.app is a GitHub source's; give write.token";

/// Why a Forgejo source can neither write nor sign an operator in.
pub const FORGEJO_UNWRITTEN: &str = "Forgejo has no API to re-run a job, nor a device flow to sign an operator in, so a Forgejo source takes neither write nor sign_in";

/// How an operator signs in at the forge to apply a re-run, by the forge's
/// OAuth device flow (ADR-0068).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignIn {
    /// The client id of the GitHub App, OAuth application or GitLab
    /// application with the device flow turned on; never that of the App
    /// a `write` names. A private GitHub repository needs a GitHub App's
    /// client id: its user token holds that App's read permissions alone.
    /// A GitHub App must be installed on the repository's account and hold
    /// no permission but read, its account permissions among them.
    pub client_id: String,
    /// The scopes asked for, each one that only reads: on GitHub
    /// `read:user` or `read:org`, none where absent; on GitLab `read_api`,
    /// `read_user`, `openid`, `profile` or `email`, `["read_api"]` where
    /// absent. The operator's token only reads who they are and what they
    /// may do on the repository; a scope that writes does not load.
    pub scopes: Option<Vec<String>>,
    /// The web root the device flow is asked at; where absent, the API's:
    /// `https://github.com` for `https://api.github.com`, a GHE.com
    /// tenant's `https://acme.ghe.com` for `https://api.acme.ghe.com`, and
    /// a GitHub Enterprise Server's or a self-managed GitLab's host for an
    /// API ending in `/api/v3` or `/api/v4`.
    pub web: Option<String>,
    /// The least the operator must be allowed on the repository: "write"
    /// (GitHub's write role, GitLab's Developer role, or above), "maintain"
    /// or "admin"; "write" where absent.
    pub least: Option<Permission>,
}

/// Whether `id` could be a client id: letters, digits, `.`, `_` and `-`.
fn client_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// The web root of the forge whose REST API is `api`.
pub(crate) fn web_of(forge: Forge, api: &str) -> String {
    let api = api.trim_end_matches('/');
    match forge {
        Forge::GitHub => api
            .strip_suffix("/api/v3")
            .map_or_else(|| api.replacen("://api.", "://", 1), ToOwned::to_owned),
        Forge::GitLab | Forge::Forgejo => crate::gitlab::web(api),
    }
}

impl SourceOptions {
    /// Whether the keys that let an operator re-run are sound on `forge`:
    /// neither on Forgejo, a write credential that is one token or one
    /// App, never the one the source reads with, and a sign-in whose
    /// client id and web root can be asked.
    pub(super) fn write_check(&self, forge: Forge) -> Result<(), String> {
        if forge == Forge::Forgejo && (self.write.is_some() || self.sign_in.is_some()) {
            return Err(FORGEJO_UNWRITTEN.to_owned());
        }
        if let Some(write) = &self.write {
            write.check()?;
            match (&write.token, &write.app) {
                (None, Some(_)) if forge != Forge::GitHub => {
                    return Err(GITHUB_APPS.to_owned());
                }
                (None, Some(app)) if self.app.as_deref() == Some(app) => {
                    return Err(format!(
                        "write.app names the App \"{app}\" the source reads as; re-runs are made as another App, installed with actions: write alone"
                    ));
                }
                (Some(token), None)
                    if self
                        .token
                        .as_ref()
                        .is_some_and(|read| read.secret == token.secret) =>
                {
                    return Err(format!(
                        "write.token names the secret {} the source reads with; give the write credential a secret of its own",
                        token.secret
                    ));
                }
                _ => {}
            }
        }
        let asked = self
            .sign_in
            .iter()
            .flat_map(|sign_in| sign_in.scopes.iter().flatten());
        if let Some(scope) = unread(forge, asked.map(String::as_str)) {
            return Err(format!(
                "sign_in.scopes names \"{scope}\", which can do more than read; the operator's token only reads who they are and their role, so give scopes among {}, or none",
                read_only(forge).join(", ")
            ));
        }
        match &self.sign_in {
            Some(sign_in) if !client_id(&sign_in.client_id) => Err(format!(
                "\"{}\" is not a client id; give it as the application's settings page shows it",
                sign_in.client_id
            )),
            Some(SignIn { web: Some(web), .. }) if !web_url(web) => Err(format!(
                "sign_in.web \"{web}\" is not a web address; give the forge's web root, such as https://github.com"
            )),
            _ => Ok(()),
        }
    }

    /// The credential the source on `forge` re-runs with, where it names
    /// one.
    #[must_use]
    pub fn writing(&self, forge: Forge) -> Option<WriteCredential<'_>> {
        let write = self.write.as_ref()?;
        if let Some(secret) = &write.token {
            return Some(WriteCredential::Token {
                secret,
                api: api_of(forge, self),
            });
        }
        let installation = match (write.installation, &self.owner, &self.repository) {
            (Some(id), _, _) => Installation::Fixed(id.get()),
            (None, Some(owner), _) => Installation::Owner(owner),
            (None, None, Some(repository)) => Installation::Repository(repository),
            (None, None, None) => return None,
        };
        Some(WriteCredential::App(Installed {
            app: write.app.as_deref()?,
            api: api_of(forge, self),
            installation,
        }))
    }

    /// How an operator signs in at the source on `forge`, or why none can.
    ///
    /// # Errors
    ///
    /// Where the forge has no device flow, or the source names no sign-in.
    pub fn signing(&self, forge: Forge) -> Result<Signing, &'static str> {
        if forge == Forge::Forgejo {
            return Err(FORGEJO_UNWRITTEN);
        }
        let sign_in = self.sign_in.as_ref().ok_or(herald_kinds::UNSIGNED)?;
        let scopes = sign_in.scopes.clone().unwrap_or_else(|| {
            let scopes: &[&str] = match forge {
                Forge::GitLab => &GITLAB_SCOPES,
                Forge::GitHub | Forge::Forgejo => &GITHUB_SCOPES,
            };
            scopes.iter().map(|scope| (*scope).to_owned()).collect()
        });
        Ok(Signing {
            web: sign_in.web.as_ref().map_or_else(
                || web_of(forge, &api_of(forge, self)),
                |web| web.trim_end_matches('/').to_owned(),
            ),
            client_id: sign_in.client_id.clone(),
            scopes,
            least: sign_in.least.unwrap_or_default(),
        })
    }
}
