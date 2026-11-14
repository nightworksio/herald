use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{GITHUB_TOKEN, Local, Opened, Opening, StoreConfig, StoreKind};
use herald_ports::RecordOptions;

use crate::{GitBranch, StoreOptions};

/// What one listing of a kind of record holds at most.
const LISTED: &str = "Each kind of record is one directory on the branch, listed in one answer of GitHub's Git Trees API, which GitHub cuts short past its own limit: a directory that outgrows it can no longer be listed. Keep few records with the store's retention, or choose a database for many.";

/// A branch of the repository, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitBranchKind {}

impl StoreKind for GitBranchKind {
    const NAME: &str = "git-branch";
    const DESCRIPTION: &str = "One file per record on a `herald-state` branch.";

    type Options = StoreOptions;
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        GitBranchKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        Some(&self.key)
    }

    fn named(&self) -> Vec<&str> {
        self.token
            .iter()
            .map(|named| named.secret.as_str())
            .collect()
    }

    fn note(&self) -> Option<&'static str> {
        Some(LISTED)
    }

    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        options: RecordOptions,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(async move {
            let repository = opening.repository(self.repository.as_deref())?;
            let name = self
                .token
                .as_ref()
                .map_or(GITHUB_TOKEN, |token| token.secret.as_str());
            let token = opening
                .secret(name, "the git-branch store needs a token that may push")
                .await?;
            let branch = GitBranch::new(
                repository,
                token,
                self.branch.as_deref(),
                self.api.as_deref(),
            );
            let http = opening.connect(branch.api())?;
            Ok(Opened::Kept(Box::new(branch.store(http).with(options))))
        })
    }
}

#[cfg(test)]
mod tests {
    use herald_core::RecordKind;

    use super::*;

    fn secret(name: &str) -> SecretRef {
        SecretRef {
            secret: name.to_owned(),
        }
    }

    #[test]
    fn its_options_are_a_git_branch_store() {
        let options = StoreOptions {
            repository: None,
            branch: None,
            token: Some(secret("T")),
            api: None,
            key: secret("K"),
        };
        assert_eq!(options.kind(), "git-branch");
        assert_eq!(GitBranchKind::NAME, "git-branch");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), vec!["T"]);
        assert_eq!(options.note(), Some(LISTED));
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
    }
}
