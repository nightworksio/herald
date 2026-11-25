use herald_core::merging::{GestureKey, GestureRecord, HandedOff, Stepped, TakenOut};
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

    async fn gesture(
        &self,
        key: &GestureKey,
    ) -> Result<Option<Versioned<GestureRecord>>, StoreError> {
        if self.fault == Fault::ReadsActions {
            return Err(broken());
        }
        self.inner.gesture(key).await
    }

    async fn put_gesture(
        &self,
        gesture: &GestureRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let settled = gesture.taken_out.last().map_or_else(
            || {
                gesture
                    .hand_off
                    .as_ref()
                    .is_some_and(|hand_off| hand_off.outcome != HandedOff::Calling)
                    || gesture
                        .own
                        .steps
                        .last()
                        .is_some_and(|step| step.outcome != Stepped::Calling)
            },
            |out| out.outcome != TakenOut::Calling,
        );
        match self.fault {
            Fault::WritesActions => Err(broken()),
            Fault::ActionsConflict => Ok(Written::Conflict),
            Fault::SettlesNoAction if settled => Err(broken()),
            _ => self.inner.put_gesture(gesture, expected).await,
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

    async fn gesture(
        &self,
        _: &GestureKey,
    ) -> Result<Option<Versioned<GestureRecord>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_gesture(
        &self,
        _: &GestureRecord,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(self.0.clone())
    }
}
