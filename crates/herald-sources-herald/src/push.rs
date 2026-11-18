//! One envelope a peer pushed into a `herald` source (ADR-0054): taken
//! as a pull takes it, its proof held to the keys that stand for the
//! instance that made it, its identity under the pushing peer's name but
//! where the peer's grant gives it as it is, and within the source's
//! budgets.

use std::fmt;

use herald_core::{Cursor, Grant, Polled};
use herald_kinds::{Pulling, Receive, Trust};
use herald_ports::Boxed;
use serde_json::Value;

use crate::options::HeraldOptions;
use crate::pull::{Pulled, Shares};

/// Why a pushed envelope is not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreceived {
    /// It cannot be read, or its proof does not hold: why.
    Unreadable(String),
    /// It came by a path the source does not take from the peer.
    Unmade,
    /// Its proof was made too far ahead of now.
    Ahead,
    /// It is of a new identity of an instance whose tombstones the source
    /// holds as many of as it may.
    Spent,
}

impl fmt::Display for Unreceived {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(why) => f.write_str(why),
            Self::Unmade => f.write_str("the envelope came by a path the source does not take"),
            Self::Ahead => f.write_str("the envelope's proof was made too far ahead of now"),
            Self::Spent => f.write_str(
                "the instance that made the envelope holds as many tombstones as the source keeps",
            ),
        }
    }
}

impl HeraldOptions {
    /// What `envelope`, which the peer `pusher` granted `grant` pushed,
    /// adds to the source's `cursor`, each key trusted as `trust` says:
    /// nothing where the cursor holds as new a proof of its identity, but
    /// where `again` says the push is worked once more after it was
    /// answered, what the cursor holds the very proof of.
    ///
    /// # Errors
    ///
    /// It cannot be read, its proof does not hold or was made too far
    /// ahead, it came by a path the source does not take, or it would take
    /// a new identity of an instance at its share.
    pub async fn received(
        &self,
        (pulling, trust, cursor): (Pulling<'_>, &Trust, Option<&Cursor>),
        (pusher, grant): (&str, &Grant),
        (envelope, again): (&Value, bool),
    ) -> Result<Polled, Unreceived> {
        let taking = Pulled::pushed(&Self {
            name: pusher.to_owned(),
            namespaces: grant.namespaces.iter().cloned().collect(),
            ..self.clone()
        });
        let mut polled = Polled {
            relisted: true,
            ..Polled::default()
        };
        let mut shares = Shares::of(cursor, self.peer_share);
        shares.retaken = again;
        taking
            .taken(
                (pulling, trust, cursor),
                (0, envelope),
                (&mut polled, &mut shares),
            )
            .await
            .map_err(|unpulled| Unreceived::Unreadable(unpulled.to_string()))?;
        if !polled.ahead.is_empty() {
            return Err(Unreceived::Ahead);
        }
        if shares.spent() {
            return Err(Unreceived::Spent);
        }
        if polled.proved.is_empty() && !polled.passed.is_empty() {
            return Err(Unreceived::Unmade);
        }
        taking.budgeted(&mut polled, trust);
        Ok(polled)
    }
}

/// The inbox of a `herald` source its peers push to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Inbox(pub(crate) HeraldOptions);

impl Receive for Inbox {
    fn remembered(&self) -> usize {
        self.0.remembered()
    }

    fn received<'a>(
        &'a self,
        reading: (Pulling<'a>, &'a Trust, Option<&'a Cursor>),
        pushed: (&'a str, &'a Grant),
        (body, again): (&'a [u8], bool),
    ) -> Boxed<'a, Result<Polled, String>> {
        Box::pin(async move {
            let envelope: Value =
                serde_json::from_slice(body).map_err(|_| "the body is no envelope".to_owned())?;
            self.0
                .received(reading, pushed, (&envelope, again))
                .await
                .map_err(|unreceived| unreceived.to_string())
        })
    }
}
