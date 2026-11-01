use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_ports::{Http, Method, Request, Response, StoreError, Text, Version};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::store::GitBranch;

const API_VERSION: &str = "2022-11-28";
const USER_AGENT: &str = concat!(
    "herald/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/nightworksio/herald)"
);
const OK: u16 = 200;
const MISSING: u16 = 404;
const CONFLICT: u16 = 409;
const UNPROCESSABLE: u16 = 422;
/// What the branch's first commit holds, so the branch has a tree.
const README: &str = "herald keeps its records on this branch, one file per record.\n";

/// A record's file as the branch holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct File {
    /// The blob a write must name to replace it.
    pub(crate) sha: String,
    /// The record's version.
    pub(crate) version: Version,
    /// The record.
    pub(crate) record: Value,
}

#[derive(Deserialize)]
struct Blob {
    sha: String,
    content: String,
}

#[derive(Deserialize)]
struct Stored {
    version: u64,
    record: Value,
}

#[derive(Deserialize)]
struct Listed {
    path: String,
}

#[derive(Deserialize)]
struct Made {
    sha: String,
}

/// What a write came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wrote {
    /// The file now holds the record.
    Written,
    /// The file changed since it was read, or exists where it was to be new.
    Conflict,
}

fn unavailable(reason: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable {
        reason: reason.to_string(),
    }
}

fn corrupt(reason: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        reason: reason.to_string(),
    }
}

fn answered(response: &Response) -> StoreError {
    unavailable(format!("GitHub answered {}", response.status))
}

impl GitBranch {
    fn request(&self, method: Method, path: &str) -> Request {
        Request::new(
            method,
            Text::plain(format!("{}/repos/{}{path}", self.api, self.repository)),
        )
        .header("Accept", Text::plain("application/vnd.github+json"))
        .header("X-GitHub-Api-Version", Text::plain(API_VERSION))
        .header("User-Agent", Text::plain(USER_AGENT))
        .header(
            "Authorization",
            Text::plain("Bearer ").then_secret(self.token.clone()),
        )
    }

    fn contents(&self, method: Method, path: &str) -> Request {
        self.request(method, &format!("/contents/{path}"))
    }

    async fn send<H: Http>(&self, http: &H, request: Request) -> Result<Response, StoreError> {
        http.send(request).await.map_err(unavailable)
    }

    /// The file at `path`, if the branch holds it.
    pub(crate) async fn read<H: Http>(
        &self,
        http: &H,
        path: &str,
    ) -> Result<Option<File>, StoreError> {
        let request = self.contents(Method::Get, &format!("{path}?ref={}", self.branch));
        let response = self.send(http, request).await?;
        if response.status == MISSING {
            return Ok(None);
        }
        if response.status != OK {
            return Err(answered(&response));
        }
        let blob: Blob = serde_json::from_slice(&response.body).map_err(corrupt)?;
        let bytes = STANDARD
            .decode(blob.content.replace('\n', ""))
            .map_err(corrupt)?;
        let stored: Stored = serde_json::from_slice(&bytes).map_err(corrupt)?;
        Ok(Some(File {
            sha: blob.sha,
            version: Version(stored.version),
            record: stored.record,
        }))
    }

    /// The paths of the files in the directory `path`.
    pub(crate) async fn list<H: Http>(
        &self,
        http: &H,
        path: &str,
    ) -> Result<Vec<String>, StoreError> {
        let request = self.contents(Method::Get, &format!("{path}?ref={}", self.branch));
        let response = self.send(http, request).await?;
        if response.status == MISSING {
            return Ok(Vec::new());
        }
        if response.status != OK {
            return Err(answered(&response));
        }
        let listed: Vec<Listed> = serde_json::from_slice(&response.body).map_err(corrupt)?;
        Ok(listed.into_iter().map(|listed| listed.path).collect())
    }

    /// Writes `record` at `version` to `path`, replacing the blob `sha`, or
    /// as a new file where `sha` is `None`.
    pub(crate) async fn write<H: Http>(
        &self,
        http: &H,
        path: &str,
        version: Version,
        record: &Value,
        sha: Option<&str>,
    ) -> Result<Wrote, StoreError> {
        let stored = json!({ "version": version.0, "record": record }).to_string();
        let mut body = json!({
            "message": format!("herald: {path}"),
            "content": STANDARD.encode(stored),
            "branch": self.branch,
        });
        if let (Some(sha), Value::Object(fields)) = (sha, &mut body) {
            fields.insert("sha".to_owned(), json!(sha));
        }
        let put = || self.contents(Method::Put, path).body(body.to_string());
        let mut response = self.send(http, put()).await?;
        if response.status == MISSING {
            self.branch_made(http).await?;
            response = self.send(http, put()).await?;
        }
        match response.status {
            _ if response.succeeded() => Ok(Wrote::Written),
            CONFLICT | UNPROCESSABLE => Ok(Wrote::Conflict),
            _ => Err(answered(&response)),
        }
    }

    /// Removes the file at `path`, whose blob is `sha`.
    pub(crate) async fn remove<H: Http>(
        &self,
        http: &H,
        path: &str,
        sha: &str,
    ) -> Result<Wrote, StoreError> {
        let body = json!({
            "message": format!("herald: {path}"),
            "sha": sha,
            "branch": self.branch,
        });
        let request = self.contents(Method::Delete, path).body(body.to_string());
        let response = self.send(http, request).await?;
        match response.status {
            OK | MISSING => Ok(Wrote::Written),
            CONFLICT | UNPROCESSABLE => Ok(Wrote::Conflict),
            _ => Err(answered(&response)),
        }
    }

    async fn made<H: Http>(
        &self,
        http: &H,
        path: &str,
        body: &Value,
    ) -> Result<Response, StoreError> {
        let request = self.request(Method::Post, path).body(body.to_string());
        self.send(http, request).await
    }

    async fn sha<H: Http>(&self, http: &H, path: &str, body: &Value) -> Result<String, StoreError> {
        let response = self.made(http, path, body).await?;
        if !response.succeeded() {
            return Err(answered(&response));
        }
        let made: Made = serde_json::from_slice(&response.body).map_err(corrupt)?;
        Ok(made.sha)
    }

    /// Makes the branch, from a commit of its own with no parent.
    async fn branch_made<H: Http>(&self, http: &H) -> Result<(), StoreError> {
        let tree = json!({
            "tree": [{ "path": "README.md", "mode": "100644", "type": "blob", "content": README }]
        });
        let tree = self.sha(http, "/git/trees", &tree).await?;
        let commit = json!({ "message": "herald: the state branch", "tree": tree, "parents": [] });
        let commit = self.sha(http, "/git/commits", &commit).await?;
        let reference = json!({ "ref": format!("refs/heads/{}", self.branch), "sha": commit });
        let response = self.made(http, "/git/refs", &reference).await?;
        if response.succeeded() || response.status == UNPROCESSABLE {
            Ok(())
        } else {
            Err(answered(&response))
        }
    }
}
