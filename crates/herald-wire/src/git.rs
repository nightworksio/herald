//! A branch written one commit at a time through GitHub's Git Data API:
//! its head read, a tree made over the head's with each file written or
//! removed, a commit of it, and the branch moved to that commit only where
//! its head is still the commit's parent (ADR-0060).

use herald_ports::{Method, Request, Secret};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::github;

/// What a file is, among a tree's entries.
const BLOB: &str = "blob";
/// The mode of a file that is not executable.
const FILE: &str = "100644";

/// A branch of a repository, reached at a GitHub API with a token that
/// may push to it.
#[derive(Debug, Clone, Copy)]
pub struct Branch<'a> {
    /// The API's base URL.
    pub api: &'a str,
    /// The repository, as `owner/name`.
    pub repository: &'a str,
    /// The branch's name.
    pub branch: &'a str,
    /// The token.
    pub token: &'a Secret,
}

/// One entry of a tree being made: a file written with its text, or
/// removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    /// Its path.
    pub path: &'a str,
    /// Its text; `None` where it is removed.
    pub text: Option<&'a str>,
}

impl Branch<'_> {
    fn request(&self, method: Method, path: &str) -> Request {
        github::request(
            method,
            format!("{}/repos/{}/git{path}", self.api, self.repository),
            self.token.clone(),
        )
    }

    /// The request reading the commit the branch's head is at.
    #[must_use]
    pub fn head(&self) -> Request {
        self.request(Method::Get, &format!("/ref/heads/{}", self.branch))
    }

    /// The request reading the commit `sha`, to learn its tree.
    #[must_use]
    pub fn commit_of(&self, sha: &str) -> Request {
        self.request(Method::Get, &format!("/commits/{sha}"))
    }

    /// The request making a tree of `entries` over the tree `base`, or
    /// over none.
    #[must_use]
    pub fn tree(&self, base: Option<&str>, entries: &[Entry<'_>]) -> Request {
        let listed: Vec<Value> = entries
            .iter()
            .map(|entry| {
                let mut listed =
                    json!({ "path": entry.path, "mode": FILE, "type": BLOB, "sha": Value::Null });
                if let (Some(text), Some(fields)) = (entry.text, listed.as_object_mut()) {
                    fields.remove("sha");
                    fields.insert("content".to_owned(), json!(text));
                }
                listed
            })
            .collect();
        let mut body = json!({ "tree": listed });
        if let (Some(base), Some(fields)) = (base, body.as_object_mut()) {
            fields.insert("base_tree".to_owned(), json!(base));
        }
        self.request(Method::Post, "/trees").body(body.to_string())
    }

    /// The request making a commit of the tree `tree`, saying `message`,
    /// whose parent is `parent`, or which has none.
    #[must_use]
    pub fn commit(&self, message: &str, tree: &str, parent: Option<&str>) -> Request {
        let body = json!({ "message": message, "tree": tree, "parents": parent.into_iter().collect::<Vec<_>>() });
        self.request(Method::Post, "/commits")
            .body(body.to_string())
    }

    /// The request moving the branch to `sha`, only where `sha` follows
    /// its head.
    #[must_use]
    pub fn moved(&self, sha: &str) -> Request {
        let body = json!({ "sha": sha, "force": false });
        self.request(Method::Patch, &format!("/refs/heads/{}", self.branch))
            .body(body.to_string())
    }

    /// The request making the branch, at `sha`.
    #[must_use]
    pub fn made(&self, sha: &str) -> Request {
        let body = json!({ "ref": format!("refs/heads/{}", self.branch), "sha": sha });
        self.request(Method::Post, "/refs").body(body.to_string())
    }
}

#[derive(Deserialize)]
struct Made {
    sha: String,
}

#[derive(Deserialize)]
struct Head {
    object: Made,
}

#[derive(Deserialize)]
struct Commit {
    tree: Made,
}

/// The SHA of what an answer says was made: a tree or a commit.
#[must_use]
pub fn made(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Made>(body)
        .ok()
        .map(|made| made.sha)
}

/// The commit an answer reading a branch's head names.
#[must_use]
pub fn head(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Head>(body)
        .ok()
        .map(|head| head.object.sha)
}

/// The tree an answer reading a commit names.
#[must_use]
pub fn tree(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Commit>(body)
        .ok()
        .map(|commit| commit.tree.sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(request: &Request) -> Value {
        serde_json::from_slice(&request.body).unwrap()
    }

    #[test]
    fn each_request_reaches_the_branch_s_git_data() {
        let token = Secret::new("GITHUB_TOKEN", "t");
        let branch = Branch {
            api: "https://api.example",
            repository: "owner/site",
            branch: "herald-pages",
            token: &token,
        };
        let head = branch.head();
        assert_eq!(head.method, Method::Get);
        assert_eq!(
            head.url.expose(),
            "https://api.example/repos/owner/site/git/ref/heads/herald-pages"
        );
        assert_eq!(
            branch.commit_of("c1").url.expose(),
            "https://api.example/repos/owner/site/git/commits/c1"
        );
        let tree = branch.tree(
            Some("t0"),
            &[
                Entry {
                    path: "a",
                    text: Some("A"),
                },
                Entry {
                    path: "b",
                    text: None,
                },
            ],
        );
        assert_eq!(tree.method, Method::Post);
        assert_eq!(
            tree.url.expose(),
            "https://api.example/repos/owner/site/git/trees"
        );
        assert_eq!(
            body(&tree),
            json!({
                "base_tree": "t0",
                "tree": [
                    { "path": "a", "mode": "100644", "type": "blob", "content": "A" },
                    { "path": "b", "mode": "100644", "type": "blob", "sha": null },
                ],
            })
        );
        assert_eq!(body(&branch.tree(None, &[])), json!({ "tree": [] }));
        let commit = branch.commit("herald: log 2", "t1", Some("c1"));
        assert_eq!(
            commit.url.expose(),
            "https://api.example/repos/owner/site/git/commits"
        );
        assert_eq!(
            body(&commit),
            json!({ "message": "herald: log 2", "tree": "t1", "parents": ["c1"] })
        );
        assert_eq!(body(&branch.commit("m", "t1", None))["parents"], json!([]));
        let moved = branch.moved("c2");
        assert_eq!(moved.method, Method::Patch);
        assert_eq!(
            moved.url.expose(),
            "https://api.example/repos/owner/site/git/refs/heads/herald-pages"
        );
        assert_eq!(body(&moved), json!({ "sha": "c2", "force": false }));
        let made = branch.made("c2");
        assert_eq!(
            made.url.expose(),
            "https://api.example/repos/owner/site/git/refs"
        );
        assert_eq!(
            body(&made),
            json!({ "ref": "refs/heads/herald-pages", "sha": "c2" })
        );
    }

    #[test]
    fn each_answer_names_what_was_made_or_nothing() {
        assert_eq!(made(br#"{"sha":"t1"}"#), Some("t1".to_owned()));
        assert_eq!(head(br#"{"object":{"sha":"c1"}}"#), Some("c1".to_owned()));
        assert_eq!(tree(br#"{"tree":{"sha":"t0"}}"#), Some("t0".to_owned()));
        for unread in [&b"{}"[..], b"no"] {
            assert_eq!(made(unread), None);
            assert_eq!(head(unread), None);
            assert_eq!(tree(unread), None);
        }
    }
}
