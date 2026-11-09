use std::any::Any;

use herald_core::{Answering, Profile, SecretRef, Symbols, WaitOptions};
use herald_kinds::{Carried, DestinationConfig, Placed, Placement, Reached, same};
use herald_ports::Secret;

use crate::interactions::{Interactions, Via};
use crate::matrix::{self, Matrix};
use crate::options::{DestinationOptions, RouteOptions};

/// The kind of a Matrix destination, as the config spells it.
pub const KIND: &str = "matrix";

impl DestinationConfig for DestinationOptions {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn symbols_written(&self) -> Option<Symbols> {
        self.symbols
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn credential(&self) -> Option<&SecretRef> {
        Some(&self.token)
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        let interactions = self.interactions.as_ref();
        [Some(&self.token)]
            .into_iter()
            .chain([
                interactions.and_then(|interactions| interactions.homeserver_token.as_ref()),
                interactions.and_then(|interactions| interactions.appservice_token.as_ref()),
            ])
            .flatten()
            .collect()
    }

    fn answering(&self) -> Option<Answering> {
        self.interactions.as_ref().map(Interactions::answering)
    }

    fn carried(&self) -> Option<Carried> {
        self.interactions.as_ref().map(|interactions| Carried {
            posted: interactions.via == Via::Appservice,
            what: "transactions the homeserver pushes",
            session: "a sync with via = \"sync\"",
        })
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        placed.of::<RouteOptions>()?;
        Some((matrix::profile(), None))
    }

    fn reach(&self, placed: &Placed, _: &str, secret: Secret) -> Option<Reached> {
        let placed = placed.of::<RouteOptions>()?;
        Some(Reached::new(
            Matrix::new(&self.homeserver, secret, placed.clone())
                .reading_back(self.read_back.unwrap_or_default())
                .waiting(self.waits()),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

impl Placement for RouteOptions {
    fn place(&self) -> Option<&str> {
        Some(&self.room)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}
