use herald_ports::{Http, Secret};
use herald_records::Kept;

/// GitHub's own API.
const API: &str = "https://api.github.com";
/// The branch the records are kept on.
const BRANCH: &str = "herald-state";

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

    /// The store kept on the branch, reached through `http`.
    #[must_use]
    pub fn store<H: Http>(self, http: H) -> GitBranchStore<H> {
        Kept::new(BranchRecords { branch: self, http })
    }
}

/// A [`GitBranch`] reached through `Http`: where a [`GitBranchStore`] keeps
/// its records.
#[derive(Debug, Clone)]
pub struct BranchRecords<H> {
    pub(crate) branch: GitBranch,
    pub(crate) http: H,
}

/// The store a [`GitBranch`] keeps.
pub type GitBranchStore<H> = Kept<BranchRecords<H>>;
