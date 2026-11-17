use super::*;

impl Announcements for Faulty {
    async fn latest(&self, project: &str) -> Result<Option<Versioned<Latest>>, StoreError> {
        self.refuses()?;
        self.inner.latest(project).await
    }

    async fn every_latest(&self) -> Result<Vec<Latest>, StoreError> {
        self.refuses()?;
        if self.fault == Fault::ListsNoLatest {
            return Ok(Vec::new());
        }
        if self.fault == Fault::ListsAnUnwrittenLatest {
            return Ok(vec![announced("unwritten", "0.1.0")]);
        }
        self.inner.every_latest().await
    }

    async fn put_latest(
        &self,
        latest: &Latest,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let expected = match self.fault {
            Fault::OverwritesLatest => self
                .inner
                .latest(&latest.project)
                .await?
                .map(|kept| kept.version),
            _ => expected,
        };
        let written = self.inner.put_latest(latest, expected).await?;
        Ok(self.raced(Fault::RacesLatest, latest.project == "raced", written))
    }
}
