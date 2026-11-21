use herald_core::{Reran, RerunRecord, RunKey};
use herald_ports::{Actions, StoreError, Version, Versioned, Written};

use super::{Failing, Fault, Refusing, broken};

impl Actions for Failing {
    async fn action(&self, key: &RunKey) -> Result<Option<Versioned<RerunRecord>>, StoreError> {
        if self.fault == Fault::ReadsActions {
            return Err(broken());
        }
        self.inner.action(key).await
    }

    async fn put_action(
        &self,
        action: &RerunRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self.fault {
            Fault::WritesActions => Err(broken()),
            Fault::ActionsConflict => Ok(Written::Conflict),
            Fault::SettlesNoAction if action.outcome != Reran::Calling => Err(broken()),
            _ => self.inner.put_action(action, expected).await,
        }
    }
}

impl Actions for Refusing {
    async fn action(&self, _: &RunKey) -> Result<Option<Versioned<RerunRecord>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_action(&self, _: &RerunRecord, _: Option<Version>) -> Result<Written, StoreError> {
        Err(self.0.clone())
    }
}
