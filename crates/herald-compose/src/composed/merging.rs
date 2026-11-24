//! `herald merge-queue`'s dry run through a composed herald: each merge
//! queue read from its forge with its source's read credential (ADR-0087).

use herald_core::Printed;
use herald_flows::{MergeQueueRead, Stop, merge_queues};
use herald_ports::{Keyring, Store};

use super::{Composed, ServingOn, Through};
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
        self.through(
            herald_flows::Admission::Once,
            self.started(),
            Queues(name, printed),
        )
        .await
    }
}
