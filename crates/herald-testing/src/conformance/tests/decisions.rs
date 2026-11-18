use herald_core::{Answer, Decided, Key};
use herald_ports::{Decisions, StoreError};

use super::{Fault, Faulty, NOON, refused};

impl Decisions for Faulty {
    async fn decision(&self, key: &Key) -> Result<Option<Decided>, StoreError> {
        self.refuses()?;
        let kept = self.inner.decision(key).await?;
        if self.fault == Fault::HoldsAnUnwrittenDecision {
            return Ok(kept.or_else(|| {
                Some(Decided {
                    key: *key,
                    task: "held".to_owned(),
                    subject: "held".to_owned(),
                    answer: Answer {
                        model: "m".to_owned(),
                        choice: None,
                        probabilities: std::collections::BTreeMap::new(),
                    },
                    at: NOON,
                })
            }));
        }
        Ok(kept)
    }

    async fn put_decision(&self, decided: &Decided) -> Result<(), StoreError> {
        self.refuses()?;
        let kept = self.inner.decision(&decided.key).await?;
        if self.fault == Fault::KeepsTheFirstDecision && kept.is_some() {
            return Ok(());
        }
        self.inner.put_decision(decided).await
    }

    async fn expire_decisions(&self, before: jiff::Timestamp) -> Result<(), StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::ExpiresNoDecision => Ok(()),
            Fault::ExpiresEveryDecision => self.inner.expire_decisions(jiff::Timestamp::MAX).await,
            _ => self.inner.expire_decisions(before).await,
        }
    }
}

#[test]
fn a_store_that_breaks_its_promises_on_decisions_is_refused() {
    refused(&[
        (
            Fault::HoldsAnUnwrittenDecision,
            "a new store holds a decision",
        ),
        (
            Fault::KeepsTheFirstDecision,
            "a decision was not the one written last under its key",
        ),
        (
            Fault::ExpiresNoDecision,
            "a decision made before the expiry was kept",
        ),
        (
            Fault::ExpiresEveryDecision,
            "a decision made at the expiry was forgotten",
        ),
    ]);
}
