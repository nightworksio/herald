//! The record of the last time each pull request left herald's own queue:
//! written once its gesture's record says it left, and put on the pull
//! request's queue entry wherever a view reads it (ADR-0087).

use std::borrow::Cow;

use herald_config::{Config, MergeQueue, ViewConfig};
use herald_core::merging::{GestureKey, Leaving, LeftKey, LeftRecord, Runs};
use herald_core::{Pull, QueueEntry};
use herald_ports::{Actions, Written};

/// Whether `store` keeps `leaving`, of the pull request the gesture
/// `gesture` was made on, or a later one, as that pull request's last:
/// written over the one kept where it is later.
pub(super) async fn told<A: Actions>(store: &A, gesture: &GestureKey, leaving: Leaving) -> bool {
    let left = LeftRecord::of(gesture, leaving);
    let Ok(kept) = store.left_record(&left.key).await else {
        return false;
    };
    if !left.replaces(kept.as_ref().map(|kept| &kept.value)) {
        return true;
    }
    let expected = kept.map(|kept| kept.version);
    matches!(
        store.put_left_record(&left, expected).await,
        Ok(Written::Written(_))
    )
}

/// The queues of `config` herald's own queue runs whose source is one of
/// `view`'s.
fn owned<'c>(config: &'c Config, view: &ViewConfig) -> Vec<&'c MergeQueue> {
    config
        .merge_queues
        .values()
        .filter(|queue| queue.runs == Runs::Herald && view.sources.contains(&queue.source))
        .collect()
}

/// The queue of `queues` that `pull` merges into.
fn queue_of<'c>(queues: &[&'c MergeQueue], pull: &Pull) -> Option<&'c MergeQueue> {
    let base = pull.base.as_deref()?;
    queues.iter().copied().find(|queue| {
        queue.branch == base
            && queue
                .forge
                .repository
                .eq_ignore_ascii_case(&pull.repository)
    })
}

/// `pulls`, read through `view`'s sources, each on the base of a queue of
/// `config` that herald's own queue runs with its last leaving of that
/// queue, as `store` keeps it, on its queue entry; each as it is where
/// `store` keeps no leaving for it or cannot be read.
pub(crate) async fn with_left<'p, A: Actions>(
    (config, store): (&Config, Option<&A>),
    view: &ViewConfig,
    pulls: &'p [Pull],
) -> Cow<'p, [Pull]> {
    let queues = owned(config, view);
    let Some(store) = store.filter(|_| !queues.is_empty()) else {
        return Cow::Borrowed(pulls);
    };
    let mut shown = pulls.to_vec();
    for pull in &mut shown {
        let Some(queue) = queue_of(&queues, pull) else {
            continue;
        };
        let key = LeftKey {
            api: queue.forge.api.clone(),
            repository: queue.forge.repository.clone(),
            number: pull.number,
        };
        if let Ok(Some(kept)) = store.left_record(&key).await {
            pull.queue = Some(QueueEntry::left_own(pull.queue.take(), kept.value.left));
        }
    }
    Cow::Owned(shown)
}
