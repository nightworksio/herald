use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{GITHUB_API_URL, Local, Opened, Opening, StoreConfig, StoreKind};
use herald_ports::RecordOptions;
use herald_wire::forgejo;

use crate::{ForgejoBranch, StoreOptions};

/// What every run keeping its records on a Forgejo branch is told.
const LISTED: &str = "Each kind of record is one directory on the branch, listed a page at a time through Forgejo's Git Trees API, up to 100000 records a directory. A write racing another on the branch is refused, and Forgejo answers that refusal 500: the run says the store refused it, and the next run writes again. Keep few records with the store's retention, or choose a database for many.";

/// The most bytes of a record it keeps: as many as the git-branch store's,
/// well within the 10 MiB of a file Forgejo answers where its instance's
/// `[api] DEFAULT_MAX_BLOB_SIZE` says nothing.
const MOST_RECORD: usize = 1_000_000;

/// A branch of a Forgejo repository, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgejoBranchKind {}

impl StoreKind for ForgejoBranchKind {
    const NAME: &str = "forgejo-branch";
    const DESCRIPTION: &str =
        "One file per record on a `herald-state` branch of a Forgejo repository.";

    type Options = StoreOptions;
}

impl StoreOptions {
    /// The branch these options name, reached through `opening`: its
    /// repository, API and token, each where given and else the run's own.
    async fn branch(&self, opening: &dyn Opening) -> Result<ForgejoBranch, String> {
        let repository = opening.repository(self.repository.as_deref())?;
        let api = match (&self.api, &self.repository) {
            (Some(api), _) => api.as_str(),
            (None, Some(_)) => forgejo::API,
            (None, None) => opening.var(GITHUB_API_URL).ok_or_else(|| {
                format!("{GITHUB_API_URL} is not set; give the store's api, or run herald in the repository's Forgejo Actions")
            })?,
        };
        let token = opening
            .token(
                self.token.as_ref(),
                "the forgejo-branch store needs a token that may push",
            )
            .await?;
        Ok(ForgejoBranch::new(
            repository,
            token,
            self.branch.as_deref(),
            api,
        ))
    }
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        ForgejoBranchKind::NAME
    }

    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        options: RecordOptions,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(async move {
            let branch = self.branch(opening.as_ref()).await?;
            let http = opening.connect(branch.api())?;
            Ok(Opened::Kept(Box::new(branch.store(http).with(options))))
        })
    }

    fn note(&self) -> Option<&'static str> {
        Some(LISTED)
    }

    fn most_record(&self) -> Option<usize> {
        Some(MOST_RECORD)
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
}

#[cfg(test)]
mod tests {
    use herald_core::RecordKind;

    use super::*;
    use crate::contents::MOST_LISTED;

    fn secret(name: &str) -> SecretRef {
        SecretRef {
            secret: name.to_owned(),
        }
    }

    #[test]
    fn its_options_are_a_forgejo_branch_store() {
        let options = StoreOptions {
            repository: None,
            branch: None,
            token: Some(secret("T")),
            api: None,
            key: secret("K"),
        };
        assert_eq!(options.kind(), "forgejo-branch");
        assert_eq!(ForgejoBranchKind::NAME, "forgejo-branch");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), vec!["T"]);
        assert_eq!(options.note(), Some(LISTED));
        assert!(LISTED.contains(&MOST_LISTED.to_string()));
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
        assert_eq!(options.most_record(), Some(MOST_RECORD));
    }
}
