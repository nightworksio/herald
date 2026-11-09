use std::any::Any;
use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{Local, Opened, Opening, StoreConfig, StoreKind, same};
use herald_ports::Secret;
use herald_records::Seal;

use crate::{RepoVariables, StoreOptions};

/// What repository variables hold at most (ADR-0005).
const CROWDED: &str = "Each record is a repository variable: a repository holds at most 500, and a run is handed at most 256 KB of them, so many records can crowd out the repository's own variables.";

/// The repository's variables, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoVariablesKind {}

impl StoreKind for RepoVariablesKind {
    const NAME: &'static str = "repo-variables";
    const DESCRIPTION: &'static str = "The repository's variables.";

    type Options = StoreOptions;
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        RepoVariablesKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        Some(&self.key)
    }

    fn named(&self) -> Vec<&str> {
        vec![self.token.secret.as_str()]
    }

    fn note(&self) -> Option<&'static str> {
        Some(CROWDED)
    }

    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        seal: Option<Secret>,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(async move {
            let repository = opening.repository(self.repository.as_deref())?;
            let token = opening
                .secret(
                    self.token.secret.as_str(),
                    "the repo-variables store needs a token with the repository's Variables permission",
                )
                .await?;
            let variables = RepoVariables::new(repository, token, self.api.as_deref());
            let http = opening.connect(variables.api())?;
            Ok(Opened::Kept(Box::new(
                variables
                    .store(http, opening.nonce())
                    .sealed(seal.map(Seal::new)),
            )))
        })
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
    fn its_options_are_a_repo_variables_store() {
        let options = StoreOptions {
            token: secret("T"),
            repository: None,
            api: None,
            key: secret("K"),
        };
        assert_eq!(options.kind(), "repo-variables");
        assert_eq!(RepoVariablesKind::NAME, "repo-variables");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), vec!["T"]);
        assert_eq!(options.note(), Some(CROWDED));
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
        assert!(options.same(options.clone().as_any()));
        assert!(!options.same(&0_u8));
    }
}
