use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{GITHUB_TOKEN, Local, Opened, Opening, StoreConfig, StoreKind};
use herald_ports::RecordOptions;

use crate::{LockLease, RepoVariables, StoreOptions};

/// What repository variables hold at most (ADR-0005).
const CROWDED: &str = "Each record is a repository variable: a repository holds at most 500, and a run is handed at most 256 KB of them, so many records can crowd out the repository's own variables. Its token needs the repository's Variables permission, which no workflow's GITHUB_TOKEN can be given.";
/// Why the job's own token keeps no records here (ADR-0002).
const UNWRITABLE: &str = "a workflow's GITHUB_TOKEN cannot write the repository's variables; give a token with the repository's Variables permission";

/// The repository's variables, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoVariablesKind {}

impl StoreKind for RepoVariablesKind {
    const NAME: &str = "repo-variables";
    const DESCRIPTION: &str = "The repository's variables.";

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

    fn withheld(&self) -> Option<(&'static str, &'static str)> {
        (self.token.secret == GITHUB_TOKEN).then_some(("token", UNWRITABLE))
    }

    fn loosened(&self) -> Vec<(&'static str, &'static str)> {
        let lease = (self.lock_lease > LockLease::DEFAULT).then_some((
            "lock_lease",
            "a lock a stopped run left holds the store for longer",
        ));
        let patience = self.lock_patience.loosened().then_some((
            "lock_patience",
            "an undated lock a stopped run left holds the store for longer",
        ));
        lease.into_iter().chain(patience).collect()
    }

    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        options: RecordOptions,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(async move {
            let repository = opening.repository(self.repository.as_deref())?;
            let token = opening
                .secret(
                    self.token.secret.as_str(),
                    "the repo-variables store needs a token with the repository's Variables permission",
                )
                .await?;
            let variables = RepoVariables::new(repository, token, self.api.as_deref())
                .locked(self.lock_lease, self.lock_patience);
            let http = opening.connect(variables.api())?;
            Ok(Opened::Kept(Box::new(
                variables.store(http, opening.nonce()).with(options),
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::LockPatience;
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
            lock_lease: LockLease::DEFAULT,
            lock_patience: LockPatience::DEFAULT,
        };
        assert_eq!(options.kind(), "repo-variables");
        assert_eq!(RepoVariablesKind::NAME, "repo-variables");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), vec!["T"]);
        assert_eq!(options.note(), Some(CROWDED));
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
        assert_eq!(options.withheld(), None);
        let job = StoreOptions {
            token: secret("GITHUB_TOKEN"),
            ..options
        };
        assert_eq!(job.withheld(), Some(("token", UNWRITABLE)));
    }

    #[test]
    fn its_lock_names_each_setting_above_its_default() {
        let options = StoreOptions {
            token: secret("T"),
            repository: None,
            api: None,
            key: secret("K"),
            lock_lease: LockLease::DEFAULT,
            lock_patience: LockPatience::DEFAULT,
        };
        assert_eq!(options.loosened(), vec![]);
        let loosened = StoreOptions {
            lock_lease: LockLease::MOST,
            lock_patience: LockPatience::MOST,
            ..options
        };
        assert_eq!(
            loosened.loosened(),
            vec![
                (
                    "lock_lease",
                    "a lock a stopped run left holds the store for longer"
                ),
                (
                    "lock_patience",
                    "an undated lock a stopped run left holds the store for longer"
                ),
            ]
        );
    }
}
