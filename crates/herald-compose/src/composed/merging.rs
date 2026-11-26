//! `herald merge-queue` through a composed herald: each merge queue read
//! from its forge with its source's read credential, and, applied, each
//! pull request due handed to the forge's queue with the queue's own write
//! credential (ADR-0087).

use std::sync::Arc;

use herald_core::Printed;
use herald_flows::{
    Admission, Handing, MergeQueueRead, Stop, merge_apply, merge_branch, merge_queues, served,
};
use herald_kinds::Branched;
use herald_ports::{Base, Keyring, Store, Timer};

use super::{Composed, ServingOn, TIMEOUT_MS, Through};
use crate::counted::Counted;
use crate::host::Host;

/// The merge queue a name names, or every one, as printed.
struct Queues<'q>(Option<&'q str>, Printed);

impl<H: Host> Through<H> for Queues<'_> {
    type Done = Result<Vec<MergeQueueRead>, Stop>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Self::Done {
        merge_queues(
            (serving.config, serving.connected, serving.ports),
            (self.0, self.1),
        )
        .await
    }
}

/// The base branch of the merge queue a name names, its failure as
/// printed.
struct Branch<'q>(&'q str, Printed);

impl<H: Host> Through<H> for Branch<'_> {
    type Done = Result<Branched, Option<String>>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Self::Done {
        merge_branch(
            (serving.config, serving.connected, serving.ports),
            (self.0, self.1),
        )
        .await
    }
}

/// The merge queue a name names, or every one, applied as printed.
struct Apply<'a, H: Host> {
    handing: &'a Handing<'a, Counted<H::Http>>,
    name: Option<&'a str>,
    printed: Printed,
}

impl<H: Host> Through<H> for Apply<'_, H> {
    type Done = Result<Vec<MergeQueueRead>, Stop>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Self::Done {
        merge_apply(
            (serving.config, serving.connected, serving.ports),
            self.handing,
            (self.name, self.printed),
        )
        .await
    }
}

impl<'o, H: Host + 'o> Composed<'o, H>
where
    H: Send,
    H::Http: Send + 'static,
    H::Clock: Send + 'static,
    H::Sql: 'static,
    H::Kv: 'static,
{
    /// The merge queue `name`, or every one, read now, a private or
    /// internal repository's entries as `printed` says; nothing is written
    /// and no write credential is read (ADR-0087).
    ///
    /// # Errors
    ///
    /// Where no merge queue has the name.
    pub async fn merge_queues(
        &self,
        (name, printed): (Option<&str>, Printed),
    ) -> Result<Vec<MergeQueueRead>, Stop> {
        self.through(Admission::Once, self.started(), Queues(name, printed))
            .await
    }

    /// The merge queue `name`, or every one, read now as
    /// [`Composed::merge_queues`] reads it, and each pull request due
    /// handed to the forge's queue with the queue's own write credential,
    /// read from the host's secrets here alone, or stepped in herald's own
    /// queue, waiting with `timer` for a head its update has not moved yet
    /// (ADR-0087).
    ///
    /// # Errors
    ///
    /// Where no merge queue has the name.
    pub async fn merge_apply<T: Timer>(
        &self,
        (name, printed): (Option<&str>, Printed),
        timer: &T,
    ) -> Result<Vec<MergeQueueRead>, Stop> {
        let connect = self.connecting();
        let handing = Handing {
            secrets: self.host.secrets(),
            connect: &connect,
            timer,
        };
        let apply = Apply::<H> {
            handing: &handing,
            name,
            printed,
        };
        self.through(Admission::Once, self.started(), apply).await
    }

    /// `work` done through everything composed, as `herald server` does
    /// it: where a merge queue's steps are the server's (`applied_by =
    /// "server"`), each round and each webhook of its source steps it with
    /// its own write credential, read from the host's secrets then alone,
    /// waiting with `timer` for a head its update has not moved yet
    /// (ADR-0087). Where none is, no write credential is reached.
    pub async fn serve<W: Through<H>, T: Timer>(&self, work: W, timer: &T) -> W::Done {
        if served(&self.config, None).is_empty() {
            return self.run(work).await;
        }
        let connect = self.connecting();
        let handing = Handing {
            secrets: self.host.secrets(),
            connect: &connect,
            timer,
        };
        self.stepping(Admission::Once, self.started(), work, Some(&handing))
            .await
    }

    /// What makes a connection reaching one base alone, counted with the
    /// run's other requests.
    fn connecting(&self) -> impl Fn(Base<'_>) -> Result<Counted<H::Http>, String> + Sync + '_ {
        let outbound = self.config.http.clone().timed(self.host.var(TIMEOUT_MS));
        move |base: Base<'_>| {
            self.host
                .connect(base, &outbound)
                .map(|http| Counted::new(http, Arc::clone(&self.made)))
        }
    }

    /// The base branch of the merge queue `name`, read now, or why it was
    /// not, as `printed` prints failures; nothing is written and no write
    /// credential is read (ADR-0087).
    ///
    /// # Errors
    ///
    /// Where the branch was not read: why, where `printed` prints
    /// failures.
    pub async fn merge_branch(
        &self,
        (name, printed): (&str, Printed),
    ) -> Result<Branched, Option<String>> {
        self.through(Admission::Once, self.started(), Branch(name, printed))
            .await
    }
}
