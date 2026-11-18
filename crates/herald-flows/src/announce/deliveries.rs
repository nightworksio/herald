//! An announcement delivered to each follower's inbox, as many at once as
//! its destination says, each delivery kept as its own record (ADR-0005).

use std::collections::BTreeSet;

use futures_util::future::join_all;
use herald_core::{Announcement, Facts, Key, Planned, Sent, State, Step, WITHDRAWN};
use herald_kinds::Signing;
use herald_ports::{Clock, Http, Inboxes, Signer, Store, Subscriptions};

use super::{Progress, Run, Stop, Unlanded, landed, stored};

impl<S: Store, C: Clock, G: Signer, H: Http> Run<'_, S, C, G, H> {
    /// Delivers to each inbox in `batch` all at once, once the run still
    /// holds the announcement, and keeps where each delivery stands as its
    /// own record.
    async fn delivered(
        &self,
        (batch, key): (&[String], Key),
        announcement: &Announcement,
        progress: &mut Progress,
    ) -> Result<(), Unlanded> {
        let now = self.ports.clock.now();
        let steps: Vec<Step> = batch.iter().cloned().map(Step::Deliver).collect();
        let unlanded = |reason| (steps.clone(), reason);
        self.write(progress).await.map_err(unlanded)?;
        for inbox in batch {
            self.ports
                .store
                .inboxes()
                .put_delivery(&key, inbox, &State::Attempted { at: now })
                .await
                .map_err(|error| unlanded(stored(error)))?;
        }
        let delivery = &self.route.config.route.delivery;
        let entry = &progress.entry;
        let sent = join_all(steps.iter().map(|step| {
            let signing = Signing {
                signer: self.ports.signer,
                now,
            };
            self.route
                .reached
                .send(self.http, signing, step, announcement, delivery, entry)
        }))
        .await;
        let mut landed = Vec::new();
        let mut failed = Vec::new();
        let mut first = None;
        for ((inbox, step), sent) in batch.iter().zip(&steps).zip(sent) {
            let reason = match sent {
                Ok(Sent::Landed(facts)) => {
                    self.ports
                        .store
                        .inboxes()
                        .put_delivery(&key, inbox, &State::Landed { facts })
                        .await
                        .map_err(|error| unlanded(stored(error)))?;
                    landed.push(step.clone());
                    continue;
                }
                Ok(Sent::RetryAfter(after)) => Stop::RetryAfter(after),
                Ok(Sent::Refused(reason)) => Stop::Refused(reason),
                Err(error) => Stop::Unreachable(error.to_string()),
            };
            failed.push(step.clone());
            first.get_or_insert(reason);
        }
        progress.landed.extend(landed);
        first.map_or(Ok(()), |reason| Err((failed, reason)))
    }

    /// Delivers the announcement, as `step` says, to the inbox of each
    /// follower of `actor` it has not landed in, as many at once as its
    /// destination's `deliveries` says, and lands `step` once every one has:
    /// the deliveries to every follower, or the updates of a revision or
    /// the Note's deletion to each the deliveries reached, each kept under a
    /// key of the revision or of the deletion.
    async fn deliveries(
        &self,
        (actor, step): (&str, &Step),
        announcement: &Announcement,
        progress: &mut Progress,
    ) -> Result<(), Unlanded> {
        let whole = |reason| (vec![step.clone()], reason);
        let created = progress.entry.key;
        let key = match step {
            Step::Updates(revision) => created.revised(revision),
            Step::Deletes => created.revised(WITHDRAWN),
            _ => created,
        };
        let mut pending = Vec::new();
        for inbox in self.recipients(actor).await.map_err(whole)? {
            let inboxes = self.ports.store.inboxes();
            let reached = key == created
                || landed(
                    inboxes
                        .delivery(&created, &inbox)
                        .await
                        .map_err(|error| whole(stored(error)))?
                        .as_ref(),
                );
            let state = inboxes
                .delivery(&key, &inbox)
                .await
                .map_err(|error| whole(stored(error)))?;
            if reached && !landed(state.as_ref()) {
                pending.push(inbox);
            }
        }
        let together = self
            .route
            .reached
            .federates()
            .map_or(1, herald_kinds::Federates::deliveries);
        let mut batches = pending.chunks(together.max(1));
        while let Some(batch) = batches.next() {
            let delivered = match self
                .ports
                .budget
                .allows(self.ports.clock.now(), batch.len())
            {
                Ok(()) => self.delivered((batch, key), announcement, progress).await,
                Err(reason) => Err((batch.iter().cloned().map(Step::Deliver).collect(), reason)),
            };
            if let Err((mut failed, reason)) = delivered {
                failed.extend(batches.flatten().cloned().map(Step::Deliver));
                return Err((failed, reason));
            }
        }
        progress.entry = progress
            .entry
            .clone()
            .landed(step.clone(), Facts::default());
        self.write(progress).await.map_err(whole)
    }

    /// Takes `step`, delivering to each follower's inbox where it is the
    /// deliveries.
    pub(super) async fn next(
        &self,
        step: &Planned,
        announcement: &mut Announcement,
        progress: &mut Progress,
    ) -> Result<(), Unlanded> {
        if let (Step::Deliveries | Step::Updates(_) | Step::Deletes, Some(actor)) =
            (&step.step, self.route.reached.follows())
        {
            return self
                .deliveries((actor, &step.step), announcement, progress)
                .await;
        }
        self.ports
            .budget
            .allows(self.ports.clock.now(), 1)
            .map_err(|reason| (vec![step.step.clone()], reason))?;
        self.take(&step.step, step.verify_first, announcement, progress)
            .await
            .map_err(|reason| (vec![step.step.clone()], reason))
    }

    /// The inboxes an announcement is delivered to: each distinct inbox of
    /// the followers of `actor` that are approved and admitted, in order.
    async fn recipients(&self, actor: &str) -> Result<Vec<String>, Stop> {
        let followers = self
            .ports
            .store
            .subscriptions()
            .subscribers(&self.route.config.route.destination, actor)
            .await
            .map_err(stored)?;
        let inboxes: BTreeSet<String> = followers
            .into_iter()
            .filter(|follower| {
                follower.pending.is_none() && self.route.reached.admits(&follower.subscriber)
            })
            .filter_map(|follower| follower.inbox)
            .collect();
        Ok(inboxes.into_iter().collect())
    }
}
