use herald_core::status;
use std::sync::{Arc, Mutex, PoisonError};

use herald_ports::{Http, Method, Response, Secret, StoreError};
use herald_records::{Blob, RecordBackend, RecordStore, Wrote};
use herald_wire::github::{self, GITHUB};
use jiff::Timestamp;
use serde::Deserialize;
use serde_json::json;

use crate::lock::{LockLease, LockPatience};
use crate::name;

/// The most variables GitHub lists in one page.
const PAGE: usize = 30;

/// Where the records are kept: a repository's Actions variables, reached
/// with a token that may write them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoVariables {
    api: String,
    repository: String,
    token: Secret,
    lease: LockLease,
    patience: LockPatience,
}

impl RepoVariables {
    /// The variables of `repository` (`owner/name`), reached at `api` or
    /// GitHub's own API with `token`.
    #[must_use]
    pub fn new(repository: &str, token: Secret, api: Option<&str>) -> Self {
        Self {
            api: github::api(api),
            repository: repository.to_owned(),
            token,
            lease: LockLease::DEFAULT,
            patience: LockPatience::DEFAULT,
        }
    }

    /// The variables, their lock held for `lease` and taken as abandoned
    /// with no time to judge it by once found held `patience` times.
    #[must_use]
    pub fn locked(self, lease: LockLease, patience: LockPatience) -> Self {
        Self {
            lease,
            patience,
            ..self
        }
    }

    /// The base URL of the API it reaches.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    /// The store kept in the variables, reached through `http`, taking its
    /// lock as `holder`, a value no other run takes it as.
    #[must_use]
    pub fn store<H: Http>(self, http: H, holder: impl Into<String>) -> RepoVariablesStore<H> {
        RecordStore::new(VariableRecords {
            variables: self,
            http,
            holder: holder.into(),
            sighted: Arc::default(),
        })
    }
}

/// [`RepoVariables`] reached through `Http`: where a [`RepoVariablesStore`]
/// keeps its records.
#[derive(Debug, Clone)]
pub struct VariableRecords<H> {
    variables: RepoVariables,
    http: H,
    holder: String,
    sighted: Arc<Mutex<Option<(Variable, usize)>>>,
}

/// What the lock is found to be.
enum Lock {
    /// No run holds it.
    Free,
    /// Another run holds it.
    Held,
    /// It is held past its lease, as it was read.
    Abandoned(Variable),
}

/// The store [`RepoVariables`] keep.
pub type RepoVariablesStore<H> = RecordStore<VariableRecords<H>>;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Variable {
    name: String,
    value: String,
    #[serde(default)]
    created_at: Option<Timestamp>,
}

#[derive(Deserialize)]
struct Listed {
    total_count: usize,
    variables: Vec<Variable>,
}

fn answered(response: &Response) -> StoreError {
    if response.status == status::FORBIDDEN {
        return StoreError::unavailable(
            "GitHub answered 403: the token cannot write the repository's variables; give the store a token with the Variables permission",
        );
    }
    StoreError::answered(GITHUB, response.status)
}

fn done(response: &Response) -> Result<Wrote, StoreError> {
    if response.succeeded() || response.status == status::NOT_FOUND {
        Ok(Wrote::Written)
    } else {
        Err(answered(response))
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
        let mut request = github::request(
            method,
            format!(
                "{}/repos/{}/actions/variables{path}",
                variables.api, variables.repository
            ),
            variables.token.clone(),
        );
        if let Some(body) = body {
            request = request.body(body.to_string());
        }
        self.http
            .send(request)
            .await
            .map_err(StoreError::unavailable)
    }

    async fn variable(&self, name: &str) -> Result<Option<(Variable, Response)>, StoreError> {
        let response = self.send(Method::Get, &format!("/{name}"), None).await?;
        if response.status == status::NOT_FOUND {
            return Ok(None);
        }
        if !response.succeeded() {
            return Err(answered(&response));
        }
        let variable = serde_json::from_slice(&response.body)
            .map_err(|error| StoreError::unreadable(name, error))?;
        Ok(Some((variable, response)))
    }

    async fn created(&self, name: &str, value: &str) -> Result<Wrote, StoreError> {
        let body = json!({ "name": name, "value": value });
        let response = self.send(Method::Post, "", Some(body)).await?;
        match response.status {
            _ if response.succeeded() => Ok(Wrote::Written),
            status::CONFLICT => Ok(Wrote::Conflict),
            _ => Err(answered(&response)),
        }
    }

    async fn page(&self, page: usize) -> Result<Listed, StoreError> {
        let response = self
            .send(Method::Get, &format!("?per_page={PAGE}&page={page}"), None)
            .await?;
        if !response.succeeded() {
            return Err(answered(&response));
        }
        serde_json::from_slice(&response.body)
            .map_err(|error| StoreError::unreadable("the repository's variables", error))
    }

    async fn deleted(&self, name: &str) -> Result<Wrote, StoreError> {
        let response = self.send(Method::Delete, &format!("/{name}"), None).await?;
        done(&response)
    }

    /// What the lock is: abandoned once held longer than its lease, by the
    /// clock of GitHub's own answer, or, where there is no time to judge
    /// by, once found held and unchanged as many times as its patience.
    async fn lock(&self) -> Result<Lock, StoreError> {
        let Some((lock, response)) = self.variable(name::LOCK).await? else {
            return Ok(Lock::Free);
        };
        let now = response.date();
        let lapsed = if let (Some(created), Some(now)) = (lock.created_at, now) {
            now.duration_since(created) > self.variables.lease.get()
        } else {
            let mut sighted = self.sighted.lock().unwrap_or_else(PoisonError::into_inner);
            let seen = match sighted.take() {
                Some((seen, times)) if seen == lock => times + 1,
                _ => 1,
            };
            *sighted = Some((lock.clone(), seen));
            seen >= self.variables.patience.get()
        };
        Ok(if lapsed {
            Lock::Abandoned(lock)
        } else {
            Lock::Held
        })
    }

    /// Whether the lock holds this store's own value.
    async fn ours(&self) -> Result<bool, StoreError> {
        Ok(self
            .variable(name::LOCK)
            .await?
            .is_some_and(|(lock, _)| lock.value == self.holder))
    }

    /// Takes the lock, unless another run holds it: an abandoned lock is
    /// deleted only while it is still the one found abandoned, and a lock
    /// is taken only once it is read back holding this store's value.
    async fn locked(&self) -> Result<bool, StoreError> {
        if self.created(name::LOCK, &self.holder).await? == Wrote::Written {
            return self.ours().await;
        }
        match self.lock().await? {
            Lock::Held => return Ok(false),
            Lock::Free => {}
            Lock::Abandoned(stale) => match self.variable(name::LOCK).await? {
                Some((lock, _)) if lock == stale => {
                    self.deleted(name::LOCK).await?;
                }
                Some(_) => return Ok(false),
                None => {}
            },
        }
        if self.created(name::LOCK, &self.holder).await? != Wrote::Written {
            return Ok(false);
        }
        self.ours().await
    }

    /// `change`'s outcome, holding the lock while it runs.
    async fn holding(&self, change: Change<'_>) -> Result<Wrote, StoreError> {
        if !self.locked().await? {
            return Ok(Wrote::Conflict);
        }
        let changed = self.change(change).await;
        let released = match self.ours().await {
            Ok(true) => self.deleted(name::LOCK).await.map(drop),
            Ok(false) => Ok(()),
            Err(error) => Err(error),
        };
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

impl<H: Http> RecordBackend for VariableRecords<H> {
    fn one_at_a_time(&self) -> bool {
        true
    }

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
        let first = self.page(1).await?;
        let mut variables = first.variables;
        for page in 2..=first.total_count.div_ceil(PAGE) {
            variables.extend(self.page(page).await?.variables);
        }
        Ok(variables
            .iter()
            .filter_map(|variable| name::path(&variable.name))
            .filter(|path| path.starts_with(&inside))
            .collect())
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
