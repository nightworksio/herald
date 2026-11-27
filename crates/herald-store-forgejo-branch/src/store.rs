use herald_ports::{Http, Secret};
use herald_records::RecordStore;

/// The branch the records are kept on.
const BRANCH: &str = "herald-state";

/// Where the records are kept: a Forgejo repository's branch, reached with
/// a token that may push to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgejoBranch {
    pub(crate) api: String,
    pub(crate) repository: String,
    pub(crate) branch: String,
    pub(crate) token: Secret,
}

impl ForgejoBranch {
    /// The branch `branch` of `repository` (`owner/name`), or
    /// `herald-state`, reached at the API `api` with `token`.
    #[must_use]
    pub fn new(repository: &str, token: Secret, branch: Option<&str>, api: &str) -> Self {
        Self {
            api: api.trim_end_matches('/').to_owned(),
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
    pub fn store<H: Http>(self, http: H) -> ForgejoBranchStore<H> {
        RecordStore::new(BranchRecords { branch: self, http })
    }
}

/// A [`ForgejoBranch`] reached through `Http`: where a
/// [`ForgejoBranchStore`] keeps its records.
#[derive(Debug, Clone)]
pub struct BranchRecords<H> {
    pub(crate) branch: ForgejoBranch,
    pub(crate) http: H,
}

/// The store a [`ForgejoBranch`] keeps.
pub type ForgejoBranchStore<H> = RecordStore<BranchRecords<H>>;
