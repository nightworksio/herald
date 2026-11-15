use herald_core::{Cursor, Listing, Membership, SourceName};
use herald_ports::{Cursors, StoreError, Version, Versioned, Written};

use super::{Fault, Faulty};

impl Cursors for Faulty {
    async fn cursor(&self, source: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        self.refuses()?;
        self.inner.cursor(source).await
    }

    async fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let expected = match self.fault {
            Fault::OverwritesCursors => self
                .inner
                .cursor(source)
                .await?
                .map(|record| record.version),
            _ => expected,
        };
        let written = self.inner.put_cursor(source, cursor, expected).await?;
        Ok(self.raced(Fault::RacesCursors, source.as_str() == "raced", written))
    }

    async fn listing(&self, source: &SourceName) -> Result<Option<Versioned<Listing>>, StoreError> {
        self.refuses()?;
        self.inner.listing(source).await
    }

    async fn put_listing(
        &self,
        source: &SourceName,
        listing: &Listing,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        self.inner.put_listing(source, listing, expected).await
    }

    async fn members(&self, source: &SourceName) -> Result<Vec<Versioned<Membership>>, StoreError> {
        self.refuses()?;
        let mut held = self.inner.members(source).await?;
        if self.fault == Fault::ListsOtherSourcesMembers {
            held.extend(
                self.inner
                    .members(&SourceName::new(super::members::OTHER))
                    .await?,
            );
        }
        Ok(held)
    }

    async fn member(
        &self,
        source: &SourceName,
        id: &str,
    ) -> Result<Option<Versioned<Membership>>, StoreError> {
        self.refuses()?;
        match self.inner.member(source, id).await? {
            None if self.fault == Fault::ReadsOtherSourcesMember => {
                self.inner
                    .member(&SourceName::new(super::members::OTHER), id)
                    .await
            }
            held => Ok(held),
        }
    }

    async fn put_member(
        &self,
        source: &SourceName,
        membership: &Membership,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let expected = if self.fault == Fault::OverwritesMembers {
            self.inner
                .members(source)
                .await?
                .into_iter()
                .find(|kept| kept.value.repository.id == membership.repository.id)
                .map(|kept| kept.version)
        } else {
            expected
        };
        self.inner.put_member(source, membership, expected).await
    }

    async fn forget_member(&self, source: &SourceName, id: &str) -> Result<(), StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::KeepsForgottenMembers => Ok(()),
            Fault::ForgetsOtherSourcesMembers => {
                self.inner.forget_member(source, id).await?;
                for member in self
                    .inner
                    .members(&SourceName::new(super::members::OTHER))
                    .await?
                {
                    self.inner
                        .forget_member(
                            &SourceName::new(super::members::OTHER),
                            &member.value.repository.id,
                        )
                        .await?;
                }
                Ok(())
            }
            _ => self.inner.forget_member(source, id).await,
        }
    }
}
