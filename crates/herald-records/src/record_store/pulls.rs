//! The pull requests kept mode keeps, one record per pull request in one
//! directory, where each source's reconcile stands (ADR-0061), and each
//! CODEOWNERS file a source read, one record per file in one directory,
//! read only under its own key (ADR-0074, ADR-0076).

use herald_core::{
    CodeownersKey, KeptCodeowners, KeptPull, PullKey, Reconciled, RepositoryKey, SourceName,
};
use herald_ports::{Pulls, StoreError, Version, Versioned, Written};
use jiff::Timestamp;

use super::RecordStore;
use crate::paths;
use crate::records::RecordBackend;

impl<R: RecordBackend> Pulls for RecordStore<R> {
    async fn pull(&self, key: &PullKey) -> Result<Option<Versioned<KeptPull>>, StoreError> {
        self.record(&paths::pull(key)).await
    }

    async fn pulls_of(
        &self,
        repository: &RepositoryKey,
    ) -> Result<Vec<Versioned<KeptPull>>, StoreError> {
        let start = paths::repository_pulls(repository);
        let mut found = Vec::new();
        for path in self.records.list(paths::PULLS).await? {
            if path.starts_with(&start) {
                found.extend(self.listed::<KeptPull>(&path).await?);
            }
        }
        Ok(found)
    }

    async fn put_pull(
        &self,
        kept: &KeptPull,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(&paths::pull(&kept.key), kept, expected)
            .await
    }

    async fn expire_pulls(&self, before: Timestamp) -> Result<(), StoreError> {
        for path in self.records.list(paths::PULLS).await? {
            let kept = self.listed::<KeptPull>(&path).await?;
            let closed = kept.and_then(|kept| kept.value.closed);
            if closed.is_some_and(|closed| closed.at < before) {
                self.removed(&path).await?;
            }
        }
        for path in self.records.list(paths::CODEOWNERS).await? {
            let kept = self.listed::<KeptCodeowners>(&path).await?;
            if kept.is_some_and(|kept| kept.value.at < before) {
                self.removed(&path).await?;
            }
        }
        Ok(())
    }

    async fn reconciled(
        &self,
        source: &SourceName,
    ) -> Result<Option<Versioned<Reconciled>>, StoreError> {
        self.record(&paths::reconciled(source)).await
    }

    async fn put_reconciled(
        &self,
        source: &SourceName,
        reconciled: &Reconciled,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(&paths::reconciled(source), reconciled, expected)
            .await
    }

    async fn codeowners(
        &self,
        key: &CodeownersKey,
    ) -> Result<Option<Versioned<KeptCodeowners>>, StoreError> {
        let kept = self
            .record::<KeptCodeowners>(&paths::codeowners(key))
            .await?;
        Ok(kept.filter(|kept| kept.value.key == *key))
    }

    async fn put_codeowners(
        &self,
        kept: &KeptCodeowners,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(&paths::codeowners(&kept.key), kept, expected)
            .await
    }
}

#[cfg(test)]
mod tests {
    use herald_core::{Codeowners, Dialect};

    use super::*;
    use crate::Memory;

    #[test]
    fn a_codeowners_record_under_another_key_s_name_reads_as_none() {
        let key = |repository: &str| CodeownersKey {
            source: SourceName::new("s"),
            repository: repository.to_owned(),
            reference: None,
        };
        let file = Codeowners::read(Dialect::GitHub, "* @o/all\n", 1);
        let other = KeptCodeowners::of(
            key("o/other"),
            ("CODEOWNERS", "\"1\""),
            &file,
            ((4096, 1), Timestamp::UNIX_EPOCH),
        )
        .unwrap();
        let store = RecordStore::new(Memory::default());
        pollster::block_on(async {
            let named = paths::codeowners(&key("o/r"));
            assert_eq!(
                store.compare_and_set(&named, &other, None).await,
                Ok(Written::Written(Version(1)))
            );
            assert_eq!(store.codeowners(&key("o/r")).await, Ok(None));
            assert_eq!(store.codeowners(&key("o/other")).await, Ok(None));
            assert_eq!(
                store.put_codeowners(&other, None).await,
                Ok(Written::Written(Version(1)))
            );
            assert!(
                store
                    .codeowners(&key("o/other"))
                    .await
                    .is_ok_and(|kept| kept.is_some_and(|kept| kept.value == other))
            );
        });
    }
}
