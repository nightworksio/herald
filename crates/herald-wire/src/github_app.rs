//! A GitHub App's installation tokens: the JWT the App asserts itself with,
//! its installation looked up, and a token minted for it (ADR-0041).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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

/// An installation, as GitHub's lookup answers it.
#[derive(Deserialize)]
struct Found {
    id: u64,
}

/// A token GitHub minted, with when it expires.
#[derive(Deserialize)]
struct Answered {
    token: String,
    expires_at: Timestamp,
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

/// The id of `installation` at `api`, asked through `http` with `jwt`:
/// `known` where the installation was found before or is named, or else
/// looked up, an owner as an organization and then as a user.
async fn installation_id(
    http: &Erased<'_, dyn DynHttp + '_>,
    (api, installation, known): (&str, Installation<'_>, Option<u64>),
    jwt: &Secret,
) -> Result<u64, Unminted> {
    let lookups = match (known, installation) {
        (Some(id), _) | (None, Installation::Fixed(id)) => return Ok(id),
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
        return read::<Found>(INSTALLATION, &answer.body).map(|found| found.id);
    }
    Err(Unminted::unreadable(&SourceError::Unreadable {
        what: INSTALLATION,
        reason: "the App is not installed there; install it on the owner or the repository"
            .to_owned(),
    }))
}

/// A token minted at `api` through `http` for `installation`, the App
/// asserting itself with `jwt`, named `name`.
///
/// # Errors
///
/// Where the installation is not found, or no token is minted for it.
pub async fn minted(
    http: &dyn DynHttp,
    (api, installation, known): (&str, Installation<'_>, Option<u64>),
    (name, jwt): (&str, &Secret),
) -> Result<InstallationToken, Unminted> {
    let http = Erased(http);
    let id = installation_id(&http, (api, installation, known), jwt).await?;
    let answer = http
        .send(asserted(
            Method::Post,
            format!("{api}/app/installations/{id}/access_tokens"),
            jwt,
        ))
        .await
        .map_err(|error| Unminted::unreachable(&error))?;
    answered(MINTED, &answer).map_err(|error| Unminted::unreadable(&error))?;
    let minted = read::<Answered>(MINTED, &answer.body)?;
    Ok(InstallationToken {
        token: Secret::new(name, minted.token),
        expires: minted.expires_at,
        installation: id,
    })
}

#[cfg(test)]
mod tests;
