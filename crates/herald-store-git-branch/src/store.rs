use herald_core::{
    Cursor, DestinationName, Entry, Key, RouteId, Schedule, SourceName, Subscription,
};
use herald_ports::{
    Capabilities, Http, Keeping, Secret, Store, StoreError, Version, Versioned, Written,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::contents::{File, Wrote};
use crate::paths;

/// GitHub's own API.
const API: &str = "https://api.github.com";
/// The branch the records are kept on.
const BRANCH: &str = "herald-state";
/// How often a write that lost a race to an unrelated write is tried again.
const ATTEMPTS: usize = 3;

/// Where the records are kept: a repository's branch, reached with a token
/// that may push to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBranch {
    pub(crate) api: String,
    pub(crate) repository: String,
    pub(crate) branch: String,
    pub(crate) token: Secret,
}

impl GitBranch {
    /// The branch `branch` of `repository` (`owner/name`), or `herald-state`,
    /// reached at `api` or GitHub's own API with `token`.
    #[must_use]
    pub fn new(repository: &str, token: Secret, branch: Option<&str>, api: Option<&str>) -> Self {
        Self {
            api: api.unwrap_or(API).trim_end_matches('/').to_owned(),
            repository: repository.to_owned(),
            branch: branch.unwrap_or(BRANCH).to_owned(),
            token,
        }
    }

    /// The base URL of the API it reaches.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }
}

/// The store a [`GitBranch`] keeps, reached through `Http`.
#[derive(Debug, Clone)]
pub struct GitBranchStore<H> {
    branch: GitBranch,
    http: H,
}

impl<H: Http> GitBranchStore<H> {
    /// The store on `branch`, reached through `http`.
    #[must_use]
    pub fn new(branch: GitBranch, http: H) -> Self {
        Self { branch, http }
    }

    async fn record<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<Option<Versioned<T>>, StoreError> {
        let Some(file) = self.branch.read(&self.http, path).await? else {
            return Ok(None);
        };
        let value = serde_json::from_value(file.record).map_err(|error| StoreError::Corrupt {
            reason: format!("{path}: {error}"),
        })?;
        Ok(Some(Versioned {
            value,
            version: file.version,
        }))
    }

    async fn compare_and_set<T: Serialize>(
        &self,
        path: &str,
        value: &T,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let record = serde_json::to_value(value).unwrap_or(Value::Null);
        for _ in 0..ATTEMPTS {
            let file = self.branch.read(&self.http, path).await?;
            if file.as_ref().map(|file| file.version) != expected {
                return Ok(Written::Conflict);
            }
            let version = Version(expected.map_or(1, |Version(number)| number + 1));
            let sha = file.as_ref().map(|file| file.sha.as_str());
            if self
                .branch
                .write(&self.http, path, version, &record, sha)
                .await?
                == Wrote::Written
            {
                return Ok(Written::Written(version));
            }
        }
        Ok(Written::Conflict)
    }

    async fn kept<T: Serialize>(&self, path: &str, value: &T) -> Result<(), StoreError> {
        let record = serde_json::to_value(value).unwrap_or(Value::Null);
        for _ in 0..ATTEMPTS {
            let file = self.branch.read(&self.http, path).await?;
            let version = file.as_ref().map_or(Version(1), |file| file.version);
            let sha = file.as_ref().map(|file| file.sha.as_str());
            if self
                .branch
                .write(&self.http, path, version, &record, sha)
                .await?
                == Wrote::Written
            {
                return Ok(());
            }
        }
        Err(StoreError::Unavailable {
            reason: format!("{path} kept changing while herald wrote it"),
        })
    }

    async fn added<T: Serialize>(&self, path: &str, value: &T) -> Result<(), StoreError> {
        if self.branch.read(&self.http, path).await?.is_some() {
            return Ok(());
        }
        let record = serde_json::to_value(value).unwrap_or(Value::Null);
        self.branch
            .write(&self.http, path, Version(1), &record, None)
            .await
            .map(|_| ())
    }

    async fn removed(&self, path: &str) -> Result<(), StoreError> {
        for _ in 0..ATTEMPTS {
            let Some(File { sha, .. }) = self.branch.read(&self.http, path).await? else {
                return Ok(());
            };
            if self.branch.remove(&self.http, path, &sha).await? == Wrote::Written {
                return Ok(());
            }
        }
        Err(StoreError::Unavailable {
            reason: format!("{path} kept changing while herald removed it"),
        })
    }

    async fn all<T: DeserializeOwned>(&self, directory: &str) -> Result<Vec<T>, StoreError> {
        let mut found = Vec::new();
        for path in self.branch.list(&self.http, directory).await? {
            if let Some(record) = self.record::<T>(&path).await? {
                found.push(record.value);
            }
        }
        Ok(found)
    }
}

impl<H: Http> Store for GitBranchStore<H> {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            ledger: Keeping::Kept,
            cursors: true,
            schedules: Keeping::Kept,
            subscriptions: true,
            pins: true,
            concurrent: true,
        }
    }

    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        self.record(&paths::entry(key)).await
    }

    async fn put_entry(
        &self,
        entry: &Entry,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(&paths::entry(&entry.key), entry, expected)
            .await
    }

    async fn cursor(&self, source: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        self.record(&paths::cursor(source)).await
    }

    async fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(&paths::cursor(source), cursor, expected)
            .await
    }

    async fn schedules(&self) -> Result<Vec<Schedule>, StoreError> {
        self.all(paths::SCHEDULES).await
    }

    async fn put_schedules(&self, add: &[Schedule], remove: &[Schedule]) -> Result<(), StoreError> {
        for schedule in remove {
            self.removed(&paths::schedule(schedule)).await?;
        }
        for schedule in add {
            self.added(&paths::schedule(schedule), schedule).await?;
        }
        Ok(())
    }

    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<String>, StoreError> {
        let subscriptions: Vec<Subscription> =
            self.all(&paths::subscribers(destination, project)).await?;
        Ok(subscriptions
            .into_iter()
            .map(|subscription| subscription.subscriber)
            .collect())
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.added(&subscribed(subscription), subscription).await
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.removed(&subscribed(subscription)).await
    }

    async fn pinned(&self, route: &RouteId) -> Result<Option<String>, StoreError> {
        Ok(self
            .record::<String>(&paths::pin(route))
            .await?
            .map(|record| record.value))
    }

    async fn put_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        self.kept(&paths::pin(route), &post).await
    }
}

fn subscribed(subscription: &Subscription) -> String {
    format!(
        "{}/{}.json",
        paths::subscribers(&subscription.destination, &subscription.project),
        paths::segment(&subscription.subscriber)
    )
}
