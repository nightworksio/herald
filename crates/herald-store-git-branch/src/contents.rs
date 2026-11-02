use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_ports::{Http, Method, Request, Response, StoreError, Text, USER_AGENT};
use herald_records::{Blob, Records, Wrote};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::store::{BranchRecords, GitBranch};

const API_VERSION: &str = "2022-11-28";
const OK: u16 = 200;
const MISSING: u16 = 404;
const CONFLICT: u16 = 409;
const UNPROCESSABLE: u16 = 422;
/// What the branch's first commit holds, so the branch has a tree.
const README: &str = "herald keeps its records on this branch, one file per record.\n";

#[derive(Deserialize)]
struct Content {
    sha: String,
    content: String,
}

#[derive(Deserialize)]
struct Listed {
    path: String,
}

#[derive(Deserialize)]
struct Made {
    sha: String,
}

fn unavailable(reason: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable {
        reason: reason.to_string(),
    }
}

fn corrupt(path: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        reason: format!("{path}: {error}"),
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
        let made: Made =
            serde_json::from_slice(&response.body).map_err(|error| corrupt(path, error))?;
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

impl<H: Http> Records for BranchRecords<H> {
    async fn read(&self, path: &str) -> Result<Option<Blob>, StoreError> {
        let branch = &self.branch;
        let request = branch.contents(Method::Get, &format!("{path}?ref={}", branch.branch));
        let response = branch.send(&self.http, request).await?;
        if response.status == MISSING {
            return Ok(None);
        }
        if response.status != OK {
            return Err(answered(&response));
        }
        let content: Content =
            serde_json::from_slice(&response.body).map_err(|error| corrupt(path, error))?;
        let bytes = STANDARD
            .decode(content.content.replace('\n', ""))
            .map_err(|error| corrupt(path, error))?;
        Ok(Some(Blob {
            token: content.sha,
            bytes,
        }))
    }

    async fn list(&self, directory: &str) -> Result<Vec<String>, StoreError> {
        let branch = &self.branch;
        let request = branch.contents(Method::Get, &format!("{directory}?ref={}", branch.branch));
        let response = branch.send(&self.http, request).await?;
        if response.status == MISSING {
            return Ok(Vec::new());
        }
        if response.status != OK {
            return Err(answered(&response));
        }
        let listed: Vec<Listed> =
            serde_json::from_slice(&response.body).map_err(|error| corrupt(directory, error))?;
        Ok(listed.into_iter().map(|listed| listed.path).collect())
    }

    async fn write(
        &self,
        path: &str,
        bytes: &[u8],
        replacing: Option<&str>,
    ) -> Result<Wrote, StoreError> {
        let branch = &self.branch;
        let mut body = json!({
            "message": format!("herald: {path}"),
            "content": STANDARD.encode(bytes),
            "branch": branch.branch,
        });
        if let (Some(sha), Value::Object(fields)) = (replacing, &mut body) {
            fields.insert("sha".to_owned(), json!(sha));
        }
        let put = || branch.contents(Method::Put, path).body(body.to_string());
        let mut response = branch.send(&self.http, put()).await?;
        if response.status == MISSING {
            branch.branch_made(&self.http).await?;
            response = branch.send(&self.http, put()).await?;
        }
        match response.status {
            _ if response.succeeded() => Ok(Wrote::Written),
            CONFLICT | UNPROCESSABLE => Ok(Wrote::Conflict),
            _ => Err(answered(&response)),
        }
    }

    async fn remove(&self, path: &str, replacing: &str) -> Result<Wrote, StoreError> {
        let branch = &self.branch;
        let body = json!({
            "message": format!("herald: {path}"),
            "sha": replacing,
            "branch": branch.branch,
        });
        let request = branch.contents(Method::Delete, path).body(body.to_string());
        let response = branch.send(&self.http, request).await?;
        match response.status {
            OK | MISSING => Ok(Wrote::Written),
            CONFLICT | UNPROCESSABLE => Ok(Wrote::Conflict),
            _ => Err(answered(&response)),
        }
    }
}
