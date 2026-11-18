//! A route's posts of a release the forge deleted, withdrawn: every
//! message herald recorded deleted where the destination can, the thread
//! archived, the pin and the Note forgotten, its place in a group taken
//! out, the feed's envelope replaced by a tombstone, and what the project
//! last announced falling back (ADR-0048).

use std::collections::BTreeSet;

use herald_config::Config;
use herald_core::{
    Announcement, Chore, Entry, Event, HeldEvent, Hold, Identity, Key, Pending, Pinning, Plan,
    Schedule, Step, WITHDRAWN, plan, reissued, withdrawal,
};
use herald_ports::{
    Announcements, Clock, Http, Ledger, Notes, Pins, Schedules, Signer, Store, Versioned, Waiting,
};

use super::{Outcome, Run, Stop, reposted, stopped, stored};
use crate::grouping::Grouped;
use crate::prepare::PreparedRoute;

/// Why a route keeps the post of a release the forge deleted.
const KEPT: &str = "the route keeps a deleted release's post, as its withdraw = \"keep\" says";

/// Why a route withdraws nothing: its destination deletes no posts.
const UNDELETABLE: &str = "the destination deletes no posts, so the post stays";

/// Why a withdrawal sends nothing: the route announced nothing of it.
const UNANNOUNCED: &str = "the route announced nothing of the release";

/// Why `route` withdraws nothing, where its destination deletes no posts:
/// a destination deletes where it edits, a herald feed by a tombstone.
pub(crate) fn undeletable(route: &PreparedRoute) -> Option<&'static str> {
    (!route.reached.profile().features.edits).then_some(UNDELETABLE)
}

/// Who asks for a release's posts to be withdrawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Asked {
    /// The forge, deleting the release: the route's `withdraw` decides.
    Forge,
    /// The operator, or a round finishing a withdrawal begun before:
    /// withdrawn whatever the route's `withdraw` says.
    Operator,
}

impl<S: Store, C: Clock, G: Signer, H: Http> Run<'_, S, C, G, H> {
    /// What the route did with `identity`, a release of `project` the forge
    /// deleted, as `asked`: each of its posts withdrawn and marked so, a
    /// re-created release announced anew; kept where the route keeps it or
    /// cannot delete. The record
    /// that lets a round finish it holds the forge's `deletion`, where it
    /// came from one.
    pub(crate) async fn withdraw(
        &self,
        config: &Config,
        (identity, project): (&Identity, Option<&str>),
        deletion: Option<HeldEvent>,
        asked: Asked,
    ) -> Outcome {
        let delivery = &self.route.config.route.delivery;
        if asked == Asked::Forge && delivery.withdraw == herald_core::Withdraw::Keep {
            return Outcome::Skipped(KEPT.to_owned());
        }
        if let Some(why) = undeletable(self.route) {
            return Outcome::Skipped(why.to_owned());
        }
        let live = match self.live(identity).await {
            Ok(live) => live,
            Err(reason) => return stopped(reason),
        };
        let hold = Hold::Withdraw {
            route: self.route.config.route.id.clone(),
            identity: identity.clone(),
        };
        if let Err(reason) = self.holding(&hold, deletion).await {
            return stopped(reason);
        }
        let reposted = reposted(&live);
        let mut landed = Vec::new();
        let mut withdrew = false;
        for target in [&live, &reposted] {
            match self.withdrawn_post(config, target).await {
                Ok(None) => {}
                Ok(Some(steps)) => {
                    landed.extend(steps);
                    withdrew = true;
                }
                Err(outcome) => return after(landed, outcome),
            }
        }
        let settled = self.settled([identity, &live], project, &hold).await;
        match (settled, withdrew) {
            (Err(reason), _) => after(landed, stopped(reason)),
            (Ok(()), false) => Outcome::Skipped(UNANNOUNCED.to_owned()),
            (Ok(()), true) => Outcome::Sent { steps: landed },
        }
    }

    /// The identity the route announces a release of `identity` under now:
    /// the first of its own and its reissued ones whose post was not
    /// withdrawn.
    pub(crate) async fn live(&self, identity: &Identity) -> Result<Identity, Stop> {
        let ledger = self.ports.store.ledger();
        let mut withdrawals: u32 = 0;
        loop {
            let live = reissued(identity, withdrawals);
            let key = Key::derive(&self.route.config.route.id, &live);
            let kept = ledger.entry(&key).await.map_err(stored)?;
            if kept.is_none_or(|kept| kept.value.withdrawn.is_none()) {
                return Ok(live);
            }
            withdrawals = withdrawals.saturating_add(1);
        }
    }

    /// `event`, under the identity the route announces it under now, where
    /// its kind may be withdrawn and the store keeps the records a
    /// withdrawal needs. A herald feed forgets a withdrawn post, so it
    /// announces a re-created release under its own identity.
    pub(crate) async fn reissued(&self, event: &Event) -> Result<Event, Stop> {
        let withdrawable = self.ports.store.waiting().is_some()
            && self.route.reached.feeds().is_none()
            && self
                .kinds
                .get(event.kind())
                .is_some_and(|spec| spec.attributes.contains_key(WITHDRAWN));
        if !withdrawable {
            return Ok(event.clone());
        }
        let live = self.live(event.identity()).await?;
        Ok(event.clone().reissued(live))
    }

    /// `hold` written, holding the forge's `deletion`, so a round finishes
    /// a withdrawal a run stopped part-way.
    async fn holding(&self, hold: &Hold, deletion: Option<HeldEvent>) -> Result<(), Stop> {
        let waiting = self.ports.store.waiting();
        if waiting.pending(hold).await.map_err(stored)?.is_some() {
            return Ok(());
        }
        let now = self.ports.clock.now();
        let pending = Pending {
            hold: hold.clone(),
            events: deletion.into_iter().collect(),
            opened: now,
            due: now,
            closed: None,
            landed: false,
        };
        waiting
            .put_pending(&pending, None)
            .await
            .map(drop)
            .map_err(stored)
    }

    /// The steps that withdrew the route's post of `identity` and its place
    /// in any group, or none where it announced and gathered nothing under
    /// it; or what stopped it.
    async fn withdrawn_post(
        &self,
        config: &Config,
        identity: &Identity,
    ) -> Result<Option<Vec<Step>>, Outcome> {
        let key = Key::derive(&self.route.config.route.id, identity);
        let record = self
            .ports
            .store
            .ledger()
            .entry(&key)
            .await
            .map_err(|error| stopped(stored(error)))?;
        let grouped = record.as_ref().and_then(|kept| kept.value.group.clone());
        let gathered = self.ungathered(config, identity, grouped).await?;
        let Some(record) = record.filter(|kept| kept.value.withdrawn.is_none()) else {
            return Ok(gathered);
        };
        let mut landed = gathered.unwrap_or_default();
        if let Some(fed) = self.route.reached.feeds() {
            if record.value.message(&Step::Post).is_none() {
                return Ok(None);
            }
            return match self.unfed(fed, identity, record).await {
                Outcome::Sent { steps } => Ok(Some(steps)),
                outcome => Err(outcome),
            };
        }
        let erased = self.erased(record).await;
        landed.extend(erased.map_err(|outcome| after(landed.clone(), outcome))?);
        Ok(Some(landed))
    }

    /// The release `identity` taken out of each group of the route that
    /// gathered it, by the records the store keeps and the group its entry
    /// names, `grouped`: the steps that landed, none where the route groups
    /// nothing of it, or what stopped it.
    async fn ungathered(
        &self,
        config: &Config,
        identity: &Identity,
        grouped: Option<String>,
    ) -> Result<Option<Vec<Step>>, Outcome> {
        let Some(grouping) = &self.route.config.route.delivery.group else {
            return Ok(None);
        };
        let route = &self.route.config.route.id;
        let every = self
            .ports
            .store
            .waiting()
            .every_pending()
            .await
            .map_err(|error| stopped(stored(error)))?;
        let holding = |pending: &Pending| {
            pending
                .events
                .iter()
                .any(|held| held.event.identity() == identity)
        };
        let mut keys: BTreeSet<String> = grouped.into_iter().collect();
        keys.extend(every.into_iter().filter_map(|kept| match kept.value.hold {
            Hold::Group { route: of, key } if of == *route && holding(&kept.value) => Some(key),
            _ => None,
        }));
        if keys.is_empty() {
            return Ok(None);
        }
        let group = Grouped {
            run: self,
            config,
            grouping,
        };
        let mut landed = Vec::new();
        for key in keys {
            let steps = group.without(&key, identity).await;
            landed.extend(steps.map_err(|outcome| after(landed.clone(), outcome))?);
        }
        Ok(Some(landed))
    }

    /// Every message of the post `record` keeps deleted, its thread
    /// archived, its pin and Note forgotten, and the post marked withdrawn:
    /// the steps that landed, or what stopped it.
    pub(crate) async fn erased(&self, record: Versioned<Entry>) -> Result<Vec<Step>, Outcome> {
        let key = record.value.key;
        let post = record.value.message(&Step::Post).map(ToOwned::to_owned);
        self.archived(&record.value).await.map_err(stopped)?;
        let wanted = withdrawal(&record.value);
        let now = self.ports.clock.now();
        let planned = match plan(Some(&record.value), &wanted, self.holder, now) {
            Plan::Held { by, until } => return Err(Outcome::Held { by, until }),
            Plan::Complete => Vec::new(),
            Plan::Send(planned) => planned,
        };
        let entry = record.value.wanting(&wanted);
        let mut progress = self
            .progressed(entry, Some(record.version))
            .await
            .map_err(stopped)?;
        let marks = self.marks(&key).await.map_err(stopped)?;
        let route = self.route.config.route.id.clone();
        let mut announcement = Announcement::withdrawing(key, marks, route, now);
        let stop = self.taken(&planned, &mut announcement, &mut progress).await;
        if stop.is_none() {
            progress.entry.withdrawn = Some(now);
        }
        let steps = match self.released(stop, progress).await {
            Outcome::Sent { steps } => steps,
            outcome => return Err(outcome),
        };
        self.forgotten(post.as_deref(), &key)
            .await
            .map_err(|reason| after(steps.clone(), stopped(reason)))?;
        Ok(steps)
    }

    /// The thread opened from the post `entry` keeps, archived at once by
    /// housekeeping, where the route archives threads and one was opened.
    async fn archived(&self, entry: &Entry) -> Result<(), Stop> {
        let thread = [Step::OpenThread, Step::Post]
            .iter()
            .find_map(|step| entry.facts(step).and_then(|facts| facts.thread.clone()));
        let archives = self
            .route
            .config
            .route
            .delivery
            .thread
            .as_ref()
            .is_some_and(|thread| thread.archive_after.is_some());
        let Some(thread) = thread.filter(|_| archives) else {
            return Ok(());
        };
        let due = Schedule {
            key: entry.key,
            route: self.route.config.route.id.clone(),
            thread,
            chore: Chore::Archive,
            due: self.ports.clock.now(),
        };
        self.ports
            .store
            .schedules()
            .put_schedules(&[due], &[])
            .await
            .map_err(stored)
    }

    /// The route's pin of `post` and the Note published as `key`
    /// forgotten, where the route keeps them.
    async fn forgotten(&self, post: Option<&str>, key: &Key) -> Result<(), Stop> {
        let route = &self.route.config.route;
        if let (Some(Pinning::Store), Some(post)) = (route.delivery.pin, post) {
            self.ports
                .store
                .pins()
                .forget_pinned(&route.id, post)
                .await
                .map_err(stored)?;
        }
        if self.route.reached.follows().is_some() {
            self.ports
                .store
                .notes()
                .forget_note(key)
                .await
                .map_err(stored)?;
        }
        Ok(())
    }

    /// Once every post of the release `identities` name is withdrawn: what
    /// `project` last announced falling back, and `hold` dropped.
    async fn settled(
        &self,
        identities: [&Identity; 2],
        project: Option<&str>,
        hold: &Hold,
    ) -> Result<(), Stop> {
        self.fell_back(identities, project).await?;
        self.ports
            .store
            .waiting()
            .drop_pending(hold)
            .await
            .map_err(stored)
    }

    /// What the project last announced, without the release `identities`
    /// name: of `project` where it is known, else of the project that
    /// keeps it.
    async fn fell_back(
        &self,
        identities: [&Identity; 2],
        project: Option<&str>,
    ) -> Result<(), Stop> {
        let Some(announcements) = self.ports.store.announcements() else {
            return Ok(());
        };
        let shows = |latest: &herald_core::Latest| identities.iter().any(|id| latest.shows(id));
        let named = if let Some(project) = project {
            project.to_owned()
        } else {
            let every = announcements.every_latest().await.map_err(stored)?;
            let Some(found) = every.into_iter().find(|latest| shows(latest)) else {
                return Ok(());
            };
            found.project
        };
        let kept = announcements.latest(&named).await.map_err(stored)?;
        let Some(kept) = kept.filter(|kept| shows(&kept.value)) else {
            return Ok(());
        };
        let project = kept.value.project.clone();
        let fallen = identities
            .iter()
            .try_fold(kept.value, |latest, id| latest.without(id));
        match fallen {
            Some(fallen) => announcements
                .put_latest(&fallen, Some(kept.version))
                .await
                .map(drop)
                .map_err(stored),
            None => announcements.forget_latest(&project).await.map_err(stored),
        }
    }
}

/// `outcome`, after `landed` landed.
fn after(mut landed: Vec<Step>, outcome: Outcome) -> Outcome {
    match outcome {
        Outcome::Stopped {
            landed: more,
            remaining,
            reason,
        } => {
            landed.extend(more);
            Outcome::Stopped {
                landed,
                remaining,
                reason,
            }
        }
        outcome => outcome,
    }
}
