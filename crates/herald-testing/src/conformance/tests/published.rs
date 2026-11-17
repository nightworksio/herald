use super::*;

impl herald_ports::Published for Faulty {
    async fn published_keys(
        &self,
    ) -> Result<Option<herald_ports::Versioned<herald_core::PublishedKeys>>, StoreError> {
        let kept = self.inner.published_keys().await?;
        Ok(kept
            .filter(|kept| self.fault != Fault::ForgetsPublishedKeys || kept.value.sequence == 1))
    }

    async fn put_published_keys(
        &self,
        keys: &herald_core::PublishedKeys,
        expected: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        let expected = match expected {
            Some(_) if self.fault == Fault::OverwritesAnyPublishedVersion => {
                self.inner.published_keys().await?.map(|kept| kept.version)
            }
            expected => expected,
        };
        self.inner.put_published_keys(keys, expected).await
    }
}

#[test]
fn a_store_that_forgets_its_published_key_set_or_writes_over_any_version_is_refused() {
    refused(&[
        (
            Fault::ForgetsPublishedKeys,
            "a published key set read back was not the one written last",
        ),
        (
            Fault::OverwritesAnyPublishedVersion,
            "a published key set was written over a version it did not expect",
        ),
    ]);
}
