use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_core::{status, unreserved};
use herald_ports::{Http, Method, Response, StoreError};
use herald_records::{Blob, RecordBackend, Wrote};
use herald_wire::forgejo::{self, FORGEJO};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::store::{BranchRecords, ForgejoBranch};

/// What a file is, among a tree's entries.
const BLOB: &str = "blob";
/// What a directory is, among a tree's entries.
const TREE: &str = "tree";
/// What a file is, as the contents API answers it.
const FILE: &str = "file";
/// How many entries of a tree one page asks for: as many as Forgejo answers
/// where its instance's `[api] DEFAULT_GIT_TREES_PER_PAGE` says nothing.
const PER_PAGE: usize = 1000;
/// The most entries one directory may hold and still be listed: as many as
/// GitHub's Git Trees API answers of a recursive tree.
pub(crate) const MOST_LISTED: usize = 100_000;

/// A file, as the contents API answers it.
#[derive(Deserialize)]
struct File {
    #[serde(rename = "type")]
    kind: String,
    sha: String,
    size: usize,
    content: Option<String>,
}

/// A branch, as the branches API answers it.
#[derive(Deserialize)]
struct Branch {
    commit: Commit,
}

#[derive(Deserialize)]
struct Commit {
    id: String,
}

/// One page of a tree, as the Git Trees API answers it.
#[derive(Deserialize)]
struct Tree {
    tree: Option<Vec<Listed>>,
    total_count: usize,
}

#[derive(Deserialize)]
struct Listed {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

impl ForgejoBranch {
    /// What Forgejo answers `method` on the repository's `path`, carrying
    /// `body` where there is one.
    async fn call<H: Http>(
        &self,
        http: &H,
        (method, path): (Method, &str),
        body: Option<&Value>,
    ) -> Result<Response, StoreError> {
        let url = format!("{}/repos/{}{path}", self.api, self.repository);
        let request = forgejo::request(method, url, self.token.clone());
        let request = match body {
            Some(body) => request.body(body.to_string()),
            None => request,
        };
        http.send(request).await.map_err(StoreError::unavailable)
    }

    /// What Forgejo answers a read of the repository's `path`, read as `T`
    /// and named `what` where it cannot be; none where Forgejo found
    /// nothing there.
    async fn answer<H: Http, T: DeserializeOwned>(
        &self,
        http: &H,
        path: &str,
        what: &str,
    ) -> Result<Option<T>, StoreError> {
        let response = self.call(http, (Method::Get, path), None).await?;
        if response.status == status::NOT_FOUND {
            return Ok(None);
        }
        if response.status != status::OK {
            return Err(StoreError::answered(FORGEJO, response.status));
        }
        serde_json::from_slice(&response.body)
            .map(Some)
            .map_err(|error| StoreError::unreadable(what, error))
    }

    /// The commit the branch is at, where it is there.
    async fn head<H: Http>(&self, http: &H) -> Result<Option<String>, StoreError> {
        let branch = self
            .branch
            .split('/')
            .map(unreserved)
            .collect::<Vec<_>>()
            .join("/");
        let branch: Option<Branch> = self
            .answer(http, &format!("/branches/{branch}"), &self.branch)
            .await?;
        Ok(branch.map(|branch| branch.commit.id))
    }

    /// Every entry of the tree or commit `sha`, read a page at a time, as
    /// `what`.
    async fn entries<H: Http>(
        &self,
        http: &H,
        sha: &str,
        what: &str,
    ) -> Result<Vec<Listed>, StoreError> {
        let mut entries = Vec::new();
        let mut total = 0;
        for page in 1.. {
            let path = format!("/git/trees/{sha}?page={page}&per_page={PER_PAGE}");
            let tree = self
                .answer::<H, Tree>(http, &path, what)
                .await?
                .ok_or_else(|| StoreError::answered(FORGEJO, status::NOT_FOUND))?;
            total = tree.total_count;
            if total > MOST_LISTED {
                return Err(StoreError::unavailable(format_args!(
                    "{what} holds {total} entries on Forgejo, more than the {MOST_LISTED} herald lists"
                )));
            }
            let listed = tree.tree.unwrap_or_default();
            if listed.is_empty() {
                break;
            }
            entries.extend(listed);
            if entries.len() >= total {
                break;
            }
        }
        if entries.len() < total {
            return Err(StoreError::unavailable(format_args!(
                "Forgejo listed {what} only in part"
            )));
        }
        Ok(entries)
    }

    /// Why a write found neither the repository nor its branch.
    fn unbranched(&self) -> StoreError {
        StoreError::unavailable(format_args!(
            "Forgejo found no repository {repository} with a branch {branch}; make the branch once, with no history, such as with `git switch --orphan {branch} && git commit --allow-empty -m \"herald: the state branch\" && git push origin {branch}`",
            repository = self.repository,
            branch = self.branch
        ))
    }
}

impl<H: Http> RecordBackend for BranchRecords<H> {
    fn one_at_a_time(&self) -> bool {
        true
    }

    async fn read(&self, path: &str) -> Result<Option<Blob>, StoreError> {
        let branch = &self.branch;
        let read = format!("/contents/{path}?ref={}", unreserved(&branch.branch));
        let Some(file) = branch.answer::<H, File>(&self.http, &read, path).await? else {
            return Ok(None);
        };
        if file.kind != FILE {
            return Err(StoreError::unreadable(
                path,
                format_args!("Forgejo holds a {} there, not a file", file.kind),
            ));
        }
        let bytes = STANDARD
            .decode(file.content.unwrap_or_default().replace('\n', ""))
            .map_err(|error| StoreError::unreadable(path, error))?;
        if bytes.len() != file.size {
            return Err(StoreError::unreadable(
                path,
                format_args!(
                    "Forgejo answered {} of its {} bytes, as a file larger than its instance's [api] DEFAULT_MAX_BLOB_SIZE",
                    bytes.len(),
                    file.size
                ),
            ));
        }
        Ok(Some(Blob {
            token: file.sha,
            bytes,
        }))
    }

    async fn list(&self, directory: &str) -> Result<Vec<String>, StoreError> {
        let branch = &self.branch;
        let Some(mut sha) = branch.head(&self.http).await? else {
            return Ok(Vec::new());
        };
        let mut reached = String::new();
        for segment in directory.split('/') {
            let what = if reached.is_empty() {
                branch.branch.clone()
            } else {
                reached.clone()
            };
            let entries = branch.entries(&self.http, &sha, &what).await?;
            let Some(found) = entries
                .into_iter()
                .find(|entry| entry.kind == TREE && entry.path == segment)
            else {
                return Ok(Vec::new());
            };
            sha = found.sha;
            reached = if reached.is_empty() {
                segment.to_owned()
            } else {
                format!("{reached}/{segment}")
            };
        }
        let entries = branch.entries(&self.http, &sha, directory).await?;
        Ok(entries
            .into_iter()
            .filter(|entry| entry.kind == BLOB)
            .map(|entry| format!("{directory}/{}", entry.path))
            .collect())
    }

    async fn write(
        &self,
        path: &str,
        bytes: &[u8],
        replacing: Option<&str>,
    ) -> Result<Wrote, StoreError> {
        let branch = &self.branch;
        let content = STANDARD.encode(bytes);
        let message = format!("herald: {path}");
        let (method, body) = match replacing {
            None => (
                Method::Post,
                json!({ "message": message, "content": content, "branch": branch.branch }),
            ),
            Some(sha) => (
                Method::Put,
                json!({ "message": message, "content": content, "branch": branch.branch, "sha": sha }),
            ),
        };
        let contents = format!("/contents/{path}");
        let response = branch
            .call(&self.http, (method, &contents), Some(&body))
            .await?;
        match (replacing, response.status) {
            (_, _) if response.succeeded() => Ok(Wrote::Written),
            (None, status::UNPROCESSABLE) | (Some(_), status::CONFLICT | status::NOT_FOUND) => {
                Ok(Wrote::Conflict)
            }
            (None, status::NOT_FOUND) => Err(branch.unbranched()),
            (_, status) => Err(StoreError::answered(FORGEJO, status)),
        }
    }

    async fn remove(&self, path: &str, replacing: &str) -> Result<Wrote, StoreError> {
        let branch = &self.branch;
        let body = json!({
            "message": format!("herald: {path}"),
            "sha": replacing,
            "branch": branch.branch,
        });
        let contents = format!("/contents/{path}");
        let response = branch
            .call(&self.http, (Method::Delete, &contents), Some(&body))
            .await?;
        match response.status {
            status::OK | status::NOT_FOUND => Ok(Wrote::Written),
            status::BAD_REQUEST => Ok(Wrote::Conflict),
            _ => Err(StoreError::answered(FORGEJO, response.status)),
        }
    }
}
