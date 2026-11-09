use std::any::Any;

use herald_core::{Answering, SecretRef, Symbols, WaitOptions};
use herald_kinds::{Carried, DestinationConfig, same};

use crate::interactions::{Interactions, Via};
use crate::options::DestinationOptions;

/// The kind of a Discord destination, as the config spells it.
pub const KIND: &str = "discord";

impl DestinationConfig for DestinationOptions {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn symbols(&self) -> Option<Symbols> {
        self.symbols
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn credential(&self) -> Option<&SecretRef> {
        self.token.as_ref().or(self.webhook.as_ref())
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        self.token.iter().chain(&self.webhook).collect()
    }

    fn answering(&self) -> Option<Answering> {
        self.interactions.as_ref().map(Interactions::answering)
    }

    fn session_domains(&self) -> Vec<&str> {
        self.interactions
            .as_ref()
            .filter(|interactions| interactions.via == Via::Gateway)
            .map(Interactions::session_domains)
            .unwrap_or_default()
    }

    fn carried(&self) -> Option<Carried> {
        self.interactions.as_ref().map(|interactions| Carried {
            posted: interactions.via == Via::Http,
            what: "interactions Discord posts",
            session: "the gateway with via = \"gateway\"",
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}
