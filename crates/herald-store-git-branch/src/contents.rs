use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_ports::{Http, Method, Request, Response, StoreError};
use herald_records::{Blob, RecordBackend, Wrote};
use herald_wire::github::{self, GITHUB};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::store::{BranchRecords, GitBranch};

const OK: u16 = 200;
const MISSING: u16 = 404;
const CONFLICT: u16 = 409;
const UNPROCESSABLE: u16 = 422;
/// What a file is, among a tree's entries.
const BLOB: &str = "blob";
/// What the branch's first commit holds, so the branch has a tree.
const README: &str = "herald keeps its records on this branch, one file per record.\n";

#[derive(Deserialize)]
struct Content {
    sha: String,
    content: String,
}

/// A directory's tree, as the Git Trees API answers it.
#[derive(Deserialize)]
struct Tree {
    tree: Vec<Listed>,
    truncated: bool,
}

#[derive(Deserialize)]
struct Listed {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Made {
    sha: String,
}

impl GitBranch {
    fn request(&self, method: Method, path: &str) -> Request {
        github::request(
            method,
            format!("{}/repos/{}{path}", self.api, self.repository),
            self.token.clone(),
        )
    }

    fn contents(&self, method: Method, path: &str) -> Request {
        self.request(method, &format!("/contents/{path}"))
    }

    async fn send<H: Http>(&self, http: &H, request: Request) -> Result<Response, StoreError> {
        http.send(request).await.map_err(StoreError::unavailable)
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
            return Err(StoreError::answered(GITHUB, response.status));
        }
        let made: Made = serde_json::from_slice(&response.body)
            .map_err(|error| StoreError::unreadable(path, error))?;
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
            Err(StoreError::answered(GITHUB, response.status))
        }
    }
}

impl<H: Http> RecordBackend for BranchRecords<H> {
    fn one_at_a_time(&self) -> bool {
        true
    }

    async fn read(&self, path: &str) -> Result<Option<Blob>, StoreError> {
        let branch = &self.branch;
        let request = branch.contents(Method::Get, &format!("{path}?ref={}", branch.branch));
        let response = branch.send(&self.http, request).await?;
        if response.status == MISSING {
            return Ok(None);
        }
        if response.status != OK {
            return Err(StoreError::answered(GITHUB, response.status));
        }
        let content: Content = serde_json::from_slice(&response.body)
            .map_err(|error| StoreError::unreadable(path, error))?;
        let bytes = STANDARD
            .decode(content.content.replace('\n', ""))
            .map_err(|error| StoreError::unreadable(path, error))?;
        Ok(Some(Blob {
            token: content.sha,
            bytes,
        }))
    }

    async fn list(&self, directory: &str) -> Result<Vec<String>, StoreError> {
        let branch = &self.branch;
        let request = branch.request(
            Method::Get,
            &format!("/git/trees/{}:{directory}", branch.branch),
        );
        let response = branch.send(&self.http, request).await?;
        if response.status == MISSING {
            return Ok(Vec::new());
        }
        if response.status != OK {
            return Err(StoreError::answered(GITHUB, response.status));
        }
        let tree: Tree = serde_json::from_slice(&response.body)
            .map_err(|error| StoreError::unreadable(directory, error))?;
        if tree.truncated {
            return Err(StoreError::unavailable(format_args!(
                "GitHub listed {directory} only in part"
            )));
        }
        Ok(tree
            .tree
            .into_iter()
            .filter(|listed| listed.kind == BLOB)
            .map(|listed| format!("{directory}/{}", listed.path))
            .collect())
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
            _ => Err(StoreError::answered(GITHUB, response.status)),
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
            _ => Err(StoreError::answered(GITHUB, response.status)),
        }
    }
}
