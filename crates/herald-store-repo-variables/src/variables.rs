use herald_ports::{Http, Method, Request, Response, Secret, StoreError, Text};
use herald_records::{Blob, Kept, Records, Wrote};
use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;
use serde_json::json;

use crate::name;

/// GitHub's own API.
const API: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
const USER_AGENT: &str = concat!(
    "herald/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/nightworksio/herald)"
);
/// The most variables GitHub lists in one page.
const PAGE: usize = 30;
/// How long a lock holds before it is taken as abandoned.
const LEASE: SignedDuration = SignedDuration::from_mins(10);
const MISSING: u16 = 404;
const FORBIDDEN: u16 = 403;
const EXISTS: u16 = 409;

/// Where the records are kept: a repository's Actions variables, reached
/// with a token that may write them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoVariables {
    api: String,
    repository: String,
    token: Secret,
}

impl RepoVariables {
    /// The variables of `repository` (`owner/name`), reached at `api` or
    /// GitHub's own API with `token`.
    #[must_use]
    pub fn new(repository: &str, token: Secret, api: Option<&str>) -> Self {
        Self {
            api: api.unwrap_or(API).trim_end_matches('/').to_owned(),
            repository: repository.to_owned(),
            token,
        }
    }

    /// The base URL of the API it reaches.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    /// The store kept in the variables, reached through `http`.
    #[must_use]
    pub fn store<H: Http>(self, http: H) -> RepoVariablesStore<H> {
        Kept::new(VariableRecords {
            variables: self,
            http,
        })
    }
}

/// [`RepoVariables`] reached through `Http`: where a [`RepoVariablesStore`]
/// keeps its records.
#[derive(Debug, Clone)]
pub struct VariableRecords<H> {
    variables: RepoVariables,
    http: H,
}

/// The store [`RepoVariables`] keep.
pub type RepoVariablesStore<H> = Kept<VariableRecords<H>>;

#[derive(Deserialize)]
struct Variable {
    name: String,
    value: String,
    #[serde(default)]
    created_at: Option<Timestamp>,
}

#[derive(Deserialize)]
struct Listed {
    variables: Vec<Variable>,
}

fn unavailable(reason: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable {
        reason: reason.to_string(),
    }
}

fn answered(response: &Response) -> StoreError {
    if response.status == FORBIDDEN {
        return unavailable(
            "GitHub answered 403: the token cannot write the repository's variables; give the store a token with the Variables permission",
        );
    }
    unavailable(format!("GitHub answered {}", response.status))
}

fn done(response: &Response) -> Result<Wrote, StoreError> {
    if response.succeeded() || response.status == MISSING {
        Ok(Wrote::Written)
    } else {
        Err(answered(response))
    }
}

fn corrupt(what: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        reason: format!("{what}: {error}"),
    }
}

impl<H: Http> VariableRecords<H> {
    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Response, StoreError> {
        let variables = &self.variables;
        let mut request = Request::new(
            method,
            Text::plain(format!(
                "{}/repos/{}/actions/variables{path}",
                variables.api, variables.repository
            )),
        )
        .header("Accept", Text::plain("application/vnd.github+json"))
        .header("X-GitHub-Api-Version", Text::plain(API_VERSION))
        .header("User-Agent", Text::plain(USER_AGENT))
        .header(
            "Authorization",
            Text::plain("Bearer ").then_secret(variables.token.clone()),
        );
        if let Some(body) = body {
            request = request.body(body.to_string());
        }
        self.http.send(request).await.map_err(unavailable)
    }

    async fn variable(&self, name: &str) -> Result<Option<(Variable, Response)>, StoreError> {
        let response = self.send(Method::Get, &format!("/{name}"), None).await?;
        if response.status == MISSING {
            return Ok(None);
        }
        if !response.succeeded() {
            return Err(answered(&response));
        }
        let variable =
            serde_json::from_slice(&response.body).map_err(|error| corrupt(name, error))?;
        Ok(Some((variable, response)))
    }

    async fn created(&self, name: &str, value: &str) -> Result<Wrote, StoreError> {
        let body = json!({ "name": name, "value": value });
        let response = self.send(Method::Post, "", Some(body)).await?;
        match response.status {
            _ if response.succeeded() => Ok(Wrote::Written),
            EXISTS => Ok(Wrote::Conflict),
            _ => Err(answered(&response)),
        }
    }

    async fn deleted(&self, name: &str) -> Result<Wrote, StoreError> {
        let response = self.send(Method::Delete, &format!("/{name}"), None).await?;
        done(&response)
    }

    /// Whether the lock is abandoned: held longer than its lease, by the
    /// clock of GitHub's own answer.
    async fn abandoned(&self) -> Result<bool, StoreError> {
        let Some((lock, response)) = self.variable(name::LOCK).await? else {
            return Ok(true);
        };
        let now = response
            .header("date")
            .and_then(|date| jiff::fmt::rfc2822::parse(date).ok())
            .map(|now| now.timestamp());
        Ok(match (lock.created_at, now) {
            (Some(created), Some(now)) => now.duration_since(created) > LEASE,
            _ => false,
        })
    }

    /// Takes the lock, unless another run holds it.
    async fn locked(&self) -> Result<bool, StoreError> {
        if self.created(name::LOCK, "held").await? == Wrote::Written {
            return Ok(true);
        }
        if !self.abandoned().await? {
            return Ok(false);
        }
        self.deleted(name::LOCK).await?;
        Ok(self.created(name::LOCK, "held").await? == Wrote::Written)
    }

    /// `change`'s outcome, holding the lock while it runs.
    async fn holding(&self, change: Change<'_>) -> Result<Wrote, StoreError> {
        if !self.locked().await? {
            return Ok(Wrote::Conflict);
        }
        let changed = self.change(change).await;
        let released = self.deleted(name::LOCK).await;
        let wrote = changed?;
        released?;
        Ok(wrote)
    }

    async fn change(&self, change: Change<'_>) -> Result<Wrote, StoreError> {
        let name = name::of(change.path);
        let current = self
            .variable(&name)
            .await?
            .map(|(variable, _)| variable.value);
        if current.as_deref() != change.replacing {
            return Ok(match (change.value, change.replacing) {
                (None, _) if current.is_none() => Wrote::Written,
                _ => Wrote::Conflict,
            });
        }
        match (change.value, current) {
            (Some(value), None) => self.created(&name, value).await,
            (Some(value), Some(_)) => {
                let body = json!({ "name": name, "value": value });
                let response = self
                    .send(Method::Patch, &format!("/{name}"), Some(body))
                    .await?;
                done(&response)
            }
            (None, _) => self.deleted(&name).await,
        }
    }
}

/// One change to make under the lock: `value` written at `path`, or the
/// record removed where `value` is `None`, if it is still `replacing`.
#[derive(Clone, Copy)]
struct Change<'a> {
    path: &'a str,
    value: Option<&'a str>,
    replacing: Option<&'a str>,
}

impl<H: Http> Records for VariableRecords<H> {
    async fn read(&self, path: &str) -> Result<Option<Blob>, StoreError> {
        Ok(self
            .variable(&name::of(path))
            .await?
            .map(|(variable, _)| Blob {
                bytes: variable.value.clone().into_bytes(),
                token: variable.value,
            }))
    }

    async fn list(&self, directory: &str) -> Result<Vec<String>, StoreError> {
        let inside = format!("{directory}/");
        let mut found = Vec::new();
        for page in 1.. {
            let response = self
                .send(Method::Get, &format!("?per_page={PAGE}&page={page}"), None)
                .await?;
            if !response.succeeded() {
                return Err(answered(&response));
            }
            let listed: Listed = serde_json::from_slice(&response.body)
                .map_err(|error| corrupt("the repository's variables", error))?;
            let count = listed.variables.len();
            found.extend(
                listed
                    .variables
                    .iter()
                    .filter_map(|variable| name::path(&variable.name))
                    .filter(|path| path.starts_with(&inside)),
            );
            if count < PAGE {
                break;
            }
        }
        Ok(found)
    }

    async fn write(
        &self,
        path: &str,
        bytes: &[u8],
        replacing: Option<&str>,
    ) -> Result<Wrote, StoreError> {
        let value = String::from_utf8_lossy(bytes);
        self.holding(Change {
            path,
            value: Some(&value),
            replacing,
        })
        .await
    }

    async fn remove(&self, path: &str, replacing: &str) -> Result<Wrote, StoreError> {
        self.holding(Change {
            path,
            value: None,
            replacing: Some(replacing),
        })
        .await
    }
}
