//! A GitHub App's installation tokens: the JWT the App asserts itself with,
//! its installation and its App looked up, and a token minted for it
//! (ADR-0041), narrowed for a re-run (ADR-0068).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::collections::BTreeMap;
use std::fmt;

use herald_core::{Installation, unreserved};
use herald_ports::{DynHttp, Erased, Http as _, HttpError, Method, Request, Secret, SecretText};
use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;

use crate::{PARSE_LIMIT, SourceError, answered, github};

/// What an installation's lookup is read as.
const INSTALLATION: &str = "the App's installation";
/// What a minted token is read as.
const MINTED: &str = "the App's installation token";
/// What the revocation of a minted token is read as.
const REVOKED: &str = "the revocation of the App's installation token";
/// The JOSE header of every JWT an App asserts itself with.
const HEADER: &str = r#"{"alg":"RS256","typ":"JWT"}"#;
/// How long before now a JWT says it was issued, for clocks that drift.
const DRIFT: SignedDuration = SignedDuration::from_secs(60);
/// How long a JWT is good for: GitHub takes at most ten minutes.
const LIFETIME: SignedDuration = SignedDuration::from_mins(9);

/// Why no token was minted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unminted {
    /// GitHub could not be reached.
    Unreachable(String),
    /// GitHub's answer minted nothing, for this reason.
    Unreadable(String),
}

/// A token GitHub minted for a narrowed mint holding more than it was asked
/// for: never used, and revoked with itself by [`Wide::revoked`] outside
/// any budget. Made only by [`narrowed`].
#[derive(Debug)]
pub struct Wide(Secret, Permissions);

impl Wide {
    /// The token revoked at `api` through `http` with itself, and dropped.
    ///
    /// # Errors
    ///
    /// Where GitHub could not be asked or did not revoke it.
    pub async fn revoked(self, http: &dyn DynHttp, api: &str) -> Result<(), Unminted> {
        revoked(http, api, &self.0).await
    }
}

impl fmt::Display for Wide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let asked = self
            .1
            .iter()
            .map(|(permission, level)| format!("{permission}: {level}"))
            .collect::<Vec<_>>()
            .join(" and ");
        write!(
            f,
            "the token GitHub minted holds other than {asked} on the one repository, so herald never uses it"
        )
    }
}

impl Unminted {
    fn unreachable(error: &HttpError) -> Self {
        Self::Unreachable(error.to_string())
    }

    fn unreadable(error: &SourceError) -> Self {
        Self::Unreadable(error.to_string())
    }
}

impl fmt::Display for Unminted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(reason) | Self::Unreadable(reason) => f.write_str(reason),
        }
    }
}

/// `T` read from `bytes`, the answer of `what`.
fn read<T: serde::de::DeserializeOwned>(what: &'static str, bytes: &[u8]) -> Result<T, Unminted> {
    crate::read(what, PARSE_LIMIT, bytes).map_err(|error| Unminted::unreadable(&error))
}

/// The permissions a narrowed token is minted with, each with its level.
type Permissions = &'static [(&'static str, &'static str)];

/// The one permission a re-run's token is minted with.
const RERUN_PERMISSIONS: Permissions = &[("actions", "write")];
/// The permissions a token that hands a pull request to the merge queue is
/// minted with (ADR-0087).
const HAND_OFF_PERMISSIONS: Permissions = &[("contents", "write"), ("pull_requests", "write")];
/// The one permission GitHub adds to every token it mints, which a narrowed
/// token may hold beside those it was minted with.
const METADATA_PERMISSION: (&str, &str) = ("metadata", "read");

/// An installation, as GitHub's lookup answers it.
#[derive(Deserialize)]
struct Looked {
    id: u64,
    app_id: Option<u64>,
}

/// A repository a minted token names.
#[derive(Deserialize)]
struct Named {
    name: String,
}

/// A token GitHub minted, with when it expires, and what it holds where
/// the answer names it.
#[derive(Deserialize)]
struct Answered {
    token: String,
    expires_at: Timestamp,
    #[serde(default)]
    permissions: BTreeMap<String, String>,
    #[serde(default)]
    repositories: Vec<Named>,
}

/// An installation GitHub named: its id, and its App's id where the answer
/// named it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    /// The installation's id.
    pub id: u64,
    /// The id of the App it is of.
    pub app: Option<u64>,
}

/// An installation token GitHub minted: the token, when it expires, and the
/// installation it is of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallationToken {
    /// The token.
    pub token: Secret,
    /// When it expires.
    pub expires: Timestamp,
    /// The id of the installation it is of.
    pub installation: u64,
    /// The id of the App it is of, where the installation's lookup named it.
    pub app: Option<u64>,
}

/// A mint narrowed to one call's permissions on one repository: the body
/// asked with, the permissions, and the repository the token must name
/// alone (ADR-0068, ADR-0087).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Narrowed {
    body: String,
    permissions: Permissions,
    repository: String,
}

impl Narrowed {
    /// The JSON body the mint is asked with.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Whether a token minted with `permissions` on `repositories` holds
    /// each permission asked, and at most [`METADATA_PERMISSION`] beside
    /// them, on this repository alone.
    fn holds(&self, permissions: &BTreeMap<String, String>, repositories: &[Named]) -> bool {
        let only = permissions.iter().all(|(named, held)| {
            let held = (named.as_str(), held.as_str());
            held == METADATA_PERMISSION || self.permissions.contains(&held)
        });
        let every = self.permissions.iter().all(|(permission, level)| {
            permissions.get(*permission).map(String::as_str) == Some(level)
        });
        let one = matches!(repositories, [named] if named.name == self.repository);
        only && every && one
    }

    /// A mint narrowed to `permissions` on the repository named `name`,
    /// without its owner, alone.
    fn of(permissions: Permissions, name: &str) -> Self {
        let asked: serde_json::Map<String, serde_json::Value> = permissions
            .iter()
            .map(|(permission, level)| ((*permission).to_owned(), (*level).into()))
            .collect();
        Self {
            body: serde_json::json!({
                "permissions": asked,
                "repositories": [name],
            })
            .to_string(),
            permissions,
            repository: name.to_owned(),
        }
    }
}

/// What the App issued by `issuer` signs at `now` to assert itself: the
/// JWT's header and claims, each base64url.
#[must_use]
pub fn assertion(issuer: &str, now: Timestamp) -> String {
    let claims = serde_json::json!({
        "iat": (now - DRIFT).as_second(),
        "exp": (now + LIFETIME).as_second(),
        "iss": issuer,
    });
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(HEADER),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

/// The JWT of `assertion` signed with `signature`, named `name`.
#[must_use]
pub fn jwt(name: &str, assertion: &str, signature: &[u8]) -> Secret {
    Secret::new(
        name,
        format!("{assertion}.{}", URL_SAFE_NO_PAD.encode(signature)),
    )
}

/// A request to `url` the App asserts itself in with `jwt`.
fn asserted(method: Method, url: String, jwt: &Secret) -> Request {
    github::headed(Request::new(method, SecretText::plain(url))).bearer(jwt.clone())
}

/// `installation` at `api`, asked through `http` with `jwt`: `known` where
/// the installation was found before or is named, its App unnamed, or else
/// looked up, an owner as an organization and then as a user, with its
/// App.
///
/// # Errors
///
/// Where the App is not installed there, or the lookup could not be made.
pub async fn installation_id(
    http: &Erased<'_, dyn DynHttp + '_>,
    (api, installation, known): (&str, Installation<'_>, Option<u64>),
    jwt: &Secret,
) -> Result<Found, Unminted> {
    let lookups = match (known, installation) {
        (Some(id), _) | (None, Installation::Fixed(id)) => return Ok(Found { id, app: None }),
        (None, Installation::Owner(owner)) => {
            let owner = unreserved(owner);
            vec![
                format!("{api}/orgs/{owner}/installation"),
                format!("{api}/users/{owner}/installation"),
            ]
        }
        (None, Installation::Repository(repository)) => {
            vec![format!("{api}/repos/{repository}/installation")]
        }
    };
    for url in lookups {
        let answer = http
            .send(asserted(Method::Get, url, jwt))
            .await
            .map_err(|error| Unminted::unreachable(&error))?;
        if answer.status == herald_core::status::NOT_FOUND {
            continue;
        }
        answered(INSTALLATION, &answer).map_err(|error| Unminted::unreadable(&error))?;
        return read::<Looked>(INSTALLATION, &answer.body).map(|looked| Found {
            id: looked.id,
            app: looked.app_id,
        });
    }
    Err(Unminted::unreadable(&SourceError::Unreadable {
        what: INSTALLATION,
        reason: "the App is not installed there; install it on the owner or the repository"
            .to_owned(),
    }))
}

/// The id of the App whose installation `id` is at `api`, asked through
/// `http` with the App's `jwt`.
///
/// # Errors
///
/// Where the installation could not be read, or names no App.
pub async fn app_of(
    http: &Erased<'_, dyn DynHttp + '_>,
    (api, id): (&str, u64),
    jwt: &Secret,
) -> Result<u64, Unminted> {
    let answer = http
        .send(asserted(
            Method::Get,
            format!("{api}/app/installations/{id}"),
            jwt,
        ))
        .await
        .map_err(|error| Unminted::unreachable(&error))?;
    answered(INSTALLATION, &answer).map_err(|error| Unminted::unreadable(&error))?;
    read::<Looked>(INSTALLATION, &answer.body)?
        .app_id
        .ok_or_else(|| {
            Unminted::Unreadable(format!("GitHub named no App for the installation {id}"))
        })
}

/// A mint narrowed to re-running jobs of the repository named `name`,
/// without its owner, alone: `actions: write` and nothing else
/// (ADR-0068).
#[must_use]
pub fn rerunning(name: &str) -> Narrowed {
    Narrowed::of(RERUN_PERMISSIONS, name)
}

/// A mint narrowed to handing pull requests of the repository named
/// `name`, without its owner, to its merge queue alone: `contents: write`
/// and `pull_requests: write` and nothing else (ADR-0087).
#[must_use]
pub fn handing_off(name: &str) -> Narrowed {
    Narrowed::of(HAND_OFF_PERMISSIONS, name)
}

/// `token`, an installation token minted at `api`, revoked through `http`
/// with itself.
///
/// # Errors
///
/// Where GitHub could not be asked or did not revoke it.
async fn revoked(http: &dyn DynHttp, api: &str, token: &Secret) -> Result<(), Unminted> {
    let http = Erased(http);
    let request = github::headed(Request::new(
        Method::Delete,
        SecretText::plain(format!("{api}/installation/token")),
    ))
    .bearer(token.clone());
    let answer = http
        .send(request)
        .await
        .map_err(|error| Unminted::unreachable(&error))?;
    answered(REVOKED, &answer).map_err(|error| Unminted::unreadable(&error))
}

/// A token minted at `api` through `http` for `installation`, the App
/// asserting itself with `jwt`, named `name`, asked for what `narrowed`
/// names where it is given; with the permissions and repositories
/// GitHub's answer says it holds.
///
/// # Errors
///
/// Where the installation is not found, or no token is minted for it.
async fn mint(
    http: &dyn DynHttp,
    (api, installation, known): (&str, Installation<'_>, Option<u64>),
    (name, jwt): (&str, &Secret),
    narrowed: Option<&Narrowed>,
) -> Result<(InstallationToken, BTreeMap<String, String>, Vec<Named>), Unminted> {
    let http = Erased(http);
    let found = installation_id(&http, (api, installation, known), jwt).await?;
    let request = asserted(
        Method::Post,
        format!("{api}/app/installations/{}/access_tokens", found.id),
        jwt,
    );
    let request = match narrowed {
        Some(narrowed) => request
            .header("Content-Type", SecretText::plain(herald_core::media::JSON))
            .body(narrowed.body.clone()),
        None => request,
    };
    let answer = http
        .send(request)
        .await
        .map_err(|error| Unminted::unreachable(&error))?;
    answered(MINTED, &answer).map_err(|error| Unminted::unreadable(&error))?;
    let Answered {
        token,
        expires_at,
        permissions,
        repositories,
    } = read::<Answered>(MINTED, &answer.body)?;
    let minted = InstallationToken {
        token: Secret::new(name, token),
        expires: expires_at,
        installation: found.id,
        app: found.app,
    };
    Ok((minted, permissions, repositories))
}

/// A token minted at `api` through `http` for `installation`, the App
/// asserting itself with `jwt`, named `name`, holding every permission of
/// the installation on each of its repositories.
///
/// # Errors
///
/// Where the installation is not found, or no token is minted for it.
pub async fn minted(
    http: &dyn DynHttp,
    installation: (&str, Installation<'_>, Option<u64>),
    asserted: (&str, &Secret),
) -> Result<InstallationToken, Unminted> {
    mint(http, installation, asserted, None)
        .await
        .map(|(minted, _, _)| minted)
}

/// A token minted at `api` through `http` for `installation`, the App
/// asserting itself with `jwt`, named `name`, holding the permissions and
/// repository `narrowed` names alone; or, where GitHub's answer says it
/// holds more, that token as [`Wide`], never to be used.
///
/// # Errors
///
/// Where the installation is not found, or no token is minted for it.
pub async fn minted_narrowed(
    http: &dyn DynHttp,
    installation: (&str, Installation<'_>, Option<u64>),
    asserted: (&str, &Secret),
    narrowed: &Narrowed,
) -> Result<Result<InstallationToken, Wide>, Unminted> {
    let (minted, permissions, repositories) =
        mint(http, installation, asserted, Some(narrowed)).await?;
    Ok(if narrowed.holds(&permissions, &repositories) {
        Ok(minted)
    } else {
        Err(Wide(minted.token, narrowed.permissions))
    })
}

#[cfg(test)]
mod tests;
