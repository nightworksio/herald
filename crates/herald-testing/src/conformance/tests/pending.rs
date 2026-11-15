use herald_core::{Hold, Pending};
use herald_ports::{StoreError, Version, Versioned, Waiting, Written};

use super::super::pending::hold;
use super::*;

impl Faulty {
    fn held(&self, hold: &Hold) -> Hold {
        match self.fault {
            Fault::SharesPending => super::super::pending::hold("shared"),
            _ => hold.clone(),
        }
    }
}

impl Waiting for Faulty {
    async fn pending(&self, hold: &Hold) -> Result<Option<Versioned<Pending>>, StoreError> {
        self.refuses()?;
        self.inner.pending(&self.held(hold)).await
    }

    async fn every_pending(&self) -> Result<Vec<Versioned<Pending>>, StoreError> {
        self.refuses()?;
        let every = self.inner.every_pending().await?;
        Ok(match self.fault {
            Fault::ListsNoPending => Vec::new(),
            Fault::HoldsAnUnwrittenPending if every.is_empty() => vec![Versioned {
                value: super::super::pending::pending("unwritten"),
                version: Version(1),
            }],
            _ => every,
        })
    }

    async fn put_pending(
        &self,
        pending: &Pending,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let current = self
            .inner
            .pending(&pending.hold)
            .await?
            .map(|kept| kept.version);
        let expected = match (self.fault, expected) {
            (Fault::OverwritesPending, _) | (Fault::IgnoresAStalePendingVersion, Some(_)) => {
                current
            }
            _ => expected,
        };
        self.inner.put_pending(pending, expected).await
    }

    async fn drop_pending(&self, hold: &Hold) -> Result<(), StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::KeepsDroppedPending => Ok(()),
            Fault::DropsEveryPending => {
                for kept in self.inner.every_pending().await? {
                    self.inner.drop_pending(&kept.value.hold).await?;
                }
                Ok(())
            }
            _ => self.inner.drop_pending(hold).await,
        }
    }
}

#[test]
fn a_store_that_breaks_its_promises_on_held_events_is_refused() {
    refused(&[
        (
            Fault::OverwritesPending,
            "held events were written over a version they did not expect",
        ),
        (
            Fault::SharesPending,
            "held events read back were not those written last under what holds them",
        ),
        (
            Fault::ListsNoPending,
            "the held events listed were not every record written, as written",
        ),
        (
            Fault::KeepsDroppedPending,
            "dropping held events did not forget them alone",
        ),
        (
            Fault::HoldsAnUnwrittenPending,
            "a new store holds events back",
        ),
        (
            Fault::IgnoresAStalePendingVersion,
            "held events were written over a version they did not expect",
        ),
        (
            Fault::DropsEveryPending,
            "dropping held events did not forget them alone",
        ),
    ]);
    assert_eq!(hold("a"), hold("a"));
}

#[test]
fn every_method_of_held_events_a_store_keeps_none_of_is_probed() {
    let mut checks = Checks::default();
    pollster::block_on(super::super::unkept::waiting(
        &Option::<&FakeStore>::None,
        &mut checks,
    ));
    assert_eq!(checks.done(), Ok(()));
    let mut checks = Checks::default();
    let store = FakeStore::default();
    pollster::block_on(super::super::unkept::waiting(&store, &mut checks));
    assert_eq!(
        checks.0,
        [
            "pending was not refused as unsupported by a store keeping none of its kind",
            "every_pending was not refused as unsupported by a store keeping none of its kind",
            "put_pending was not refused as unsupported by a store keeping none of its kind",
            "drop_pending was not refused as unsupported by a store keeping none of its kind",
        ]
    );
}
