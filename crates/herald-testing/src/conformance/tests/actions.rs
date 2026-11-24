use herald_core::merging::{GestureKey, GestureRecord};
use herald_core::{Reran, RerunRecord, RunKey};
use herald_ports::{Actions, StoreError, Version, Versioned, Written};

use super::{Fault, Faulty, refused};
use crate::conformance::NOON;
use crate::conformance::actions::{action, gestured, run};

impl Actions for Faulty {
    async fn action(&self, key: &RunKey) -> Result<Option<Versioned<RerunRecord>>, StoreError> {
        self.refuses()?;
        let asked = match self.fault {
            Fault::SharesActionsAcrossHeads => run(&key.run, &key.job, "abc"),
            Fault::SharesActionsAcrossJobs => run(&key.run, "70", &key.head),
            _ => key.clone(),
        };
        let read = self.inner.action(&asked).await?;
        if self.fault == Fault::HoldsAnUnwrittenAction && read.is_none() {
            return Ok(Some(Versioned {
                value: action(key.clone(), Reran::Rerun),
                version: Version(1),
            }));
        }
        Ok(read)
    }

    async fn put_action(
        &self,
        action: &RerunRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let current = self.inner.action(&action.key).await?;
        match self.fault {
            Fault::OverwritesAnyActionVersion if expected.is_none() => {
                let version = current.map(|current| current.version);
                self.inner.put_action(action, version).await
            }
            Fault::OverwritesAStaleActionVersion if expected.is_some() => {
                let version = current.map(|current| current.version);
                self.inner.put_action(action, version).await
            }
            Fault::KeepsTheFirstAction if expected.is_some() => match current {
                Some(current) => self.inner.put_action(&current.value, expected).await,
                None => self.inner.put_action(action, expected).await,
            },
            _ => self.inner.put_action(action, expected).await,
        }
    }

    async fn gesture(
        &self,
        key: &GestureKey,
    ) -> Result<Option<Versioned<GestureRecord>>, StoreError> {
        self.refuses()?;
        let asked = match self.fault {
            Fault::SharesGesturesAcrossTimes => gestured(0, NOON),
            _ => key.clone(),
        };
        self.inner.gesture(&asked).await
    }

    async fn put_gesture(
        &self,
        gesture: &GestureRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::OverwritesAnyGestureVersion if expected.is_none() => {
                let current = self.inner.gesture(&gesture.key).await?;
                let version = current.map(|current| current.version);
                self.inner.put_gesture(gesture, version).await
            }
            _ => self.inner.put_gesture(gesture, expected).await,
        }
    }
}

#[test]
fn a_store_that_breaks_its_promises_on_the_record_of_a_re_run_is_refused() {
    refused(&[
        (
            Fault::HoldsAnUnwrittenAction,
            "a new store holds the record of a re-run",
        ),
        (
            Fault::OverwritesAnyActionVersion,
            "the record of a re-run was written as new over one that exists",
        ),
        (
            Fault::OverwritesAStaleActionVersion,
            "the record of a re-run was written over a stale version",
        ),
        (
            Fault::KeepsTheFirstAction,
            "the record of a re-run did not read back as last written",
        ),
        (
            Fault::SharesActionsAcrossHeads,
            "the record of a re-run on one head was read for another",
        ),
        (
            Fault::SharesActionsAcrossJobs,
            "the record of a re-run of one job was read for another",
        ),
    ]);
}

#[test]
fn a_store_that_breaks_its_promises_on_the_record_of_a_gesture_is_refused() {
    refused(&[
        (
            Fault::OverwritesAnyGestureVersion,
            "the record of a gesture was written as new over one that exists",
        ),
        (
            Fault::SharesGesturesAcrossTimes,
            "the record of one gesture was read for a later one",
        ),
    ]);
}
