use std::any::Any;

use herald_core::SecretRef;
use herald_kinds::{StoreConfig, StoreKind, same};

use crate::StoreOptions;

/// A branch of the repository, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitBranchKind {}

impl StoreKind for GitBranchKind {
    const NAME: &'static str = "git-branch";
    const DESCRIPTION: &'static str = "One file per record on a `herald-state` branch.";

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

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
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
        assert_eq!(options.note(), None);
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
        assert!(options.same(options.clone().as_any()));
        assert!(!options.same(&0_u8));
    }
}
