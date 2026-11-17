use super::*;

impl herald_ports::Tokens for Faulty {
    async fn token(
        &self,
        name: &str,
    ) -> Result<Option<herald_ports::Versioned<herald_core::KeptToken>>, StoreError> {
        let kept = self.inner.token(name).await?;
        Ok(kept.filter(|_| self.fault != Fault::ForgetsTheSecondToken || !name.ends_with("/2")))
    }

    async fn put_token(
        &self,
        token: &herald_core::KeptToken,
        expected: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        let expected = match expected {
            Some(_) if self.fault == Fault::OverwritesAnyTokenVersion => self
                .inner
                .token(&token.name)
                .await?
                .map(|kept| kept.version),
            expected => expected,
        };
        self.inner.put_token(token, expected).await
    }
}

#[test]
fn a_store_that_forgets_a_token_or_writes_over_any_version_is_refused() {
    refused(&[
        (
            Fault::ForgetsTheSecondToken,
            "a token read back was not the one written last under its name",
        ),
        (
            Fault::OverwritesAnyTokenVersion,
            "a token was written over a version it did not expect",
        ),
    ]);
}
