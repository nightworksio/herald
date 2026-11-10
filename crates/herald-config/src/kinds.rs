use std::fmt;
use std::marker::PhantomData;
use std::ops::Range;

use herald_kinds::{Destination, DestinationKind, Fault, Placed, Placing, Store, StoreKind};
use jiff::SignedDuration;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::DeserializeOwned;

use crate::load::{Loader, Value};
use crate::schema::{routed, store, tagged};

/// Every kind of destination and store a herald reads its config with: the
/// one list each host composes, which the loader parses through and the
/// schema is generated from.
#[derive(Default)]
pub struct Kinds {
    destinations: Vec<Box<dyn Kinded>>,
    stores: Vec<Box<dyn StoreKinded>>,
}

impl Kinds {
    /// No kind at all.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// These kinds, and destinations of kind `K`.
    #[must_use]
    pub fn destination<K>(mut self) -> Self
    where
        K: DestinationKind,
        K::Options: DeserializeOwned + JsonSchema,
        K::Route: DeserializeOwned + JsonSchema,
    {
        self.destinations.push(Box::new(Of::<K>(PhantomData)));
        self
    }

    /// Every destination kind's name, as `kind` writes it.
    #[must_use]
    pub fn destination_kinds(&self) -> Vec<&'static str> {
        self.destinations.iter().map(|kind| kind.name()).collect()
    }

    pub(crate) fn destination_named(&self, name: &str) -> Option<&dyn Kinded> {
        self.destinations
            .iter()
            .map(AsRef::as_ref)
            .find(|kind| kind.name() == name)
    }

    pub(crate) fn destinations(&self) -> impl Iterator<Item = &dyn Kinded> {
        self.destinations.iter().map(AsRef::as_ref)
    }

    /// These kinds, and stores of kind `K`.
    #[must_use]
    pub fn store<K>(mut self) -> Self
    where
        K: StoreKind,
        K::Options: DeserializeOwned + JsonSchema,
    {
        self.stores.push(Box::new(Of::<K>(PhantomData)));
        self
    }

    /// Every store kind's name, as `kind` writes it.
    #[must_use]
    pub fn store_kinds(&self) -> Vec<&'static str> {
        self.stores.iter().map(|kind| kind.name()).collect()
    }

    pub(crate) fn store_named(&self, name: &str) -> Option<&dyn StoreKinded> {
        self.stores
            .iter()
            .map(AsRef::as_ref)
            .find(|kind| kind.name() == name)
    }

    pub(crate) fn stores(&self) -> impl Iterator<Item = &dyn StoreKinded> {
        self.stores.iter().map(AsRef::as_ref)
    }
}

impl fmt::Debug for Kinds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Kinds")
            .field("destinations", &self.destination_kinds())
            .field("stores", &self.store_kinds())
            .finish()
    }
}

/// Where a table is written: its path and its span.
#[derive(Clone, Copy)]
pub(crate) struct At<'a> {
    pub(crate) path: &'a str,
    pub(crate) span: &'a Range<usize>,
}

/// One destination kind, its types erased.
pub(crate) trait Kinded: Send + Sync {
    fn name(&self) -> &'static str;

    fn parse(
        &self,
        loader: &mut Loader<'_>,
        rest: Value<'_>,
        at: At<'_>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Option<Destination>;

    fn place(
        &self,
        loader: &mut Loader<'_>,
        destination: &Destination,
        rest: Value<'_>,
        at: At<'_>,
        placing: Placing<'_>,
    ) -> Option<(Placed, &'static str, bool)>;

    fn outlasts(&self, destination: &Destination, total: SignedDuration) -> Option<Fault>;

    fn destination_schema(&self, generator: &mut SchemaGenerator) -> Schema;

    fn route_schema(&self, generator: &mut SchemaGenerator) -> Schema;
}

struct Of<K>(PhantomData<fn() -> K>);

impl<K> Kinded for Of<K>
where
    K: DestinationKind,
    K::Options: DeserializeOwned + JsonSchema,
    K::Route: DeserializeOwned + JsonSchema,
{
    fn name(&self) -> &'static str {
        K::NAME
    }

    fn parse(
        &self,
        loader: &mut Loader<'_>,
        rest: Value<'_>,
        at: At<'_>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Option<Destination> {
        let options = loader.typed::<K::Options>(rest, at.path)?;
        admitted(
            loader,
            at,
            K::checked(options, read).map(|(options, faults)| (Destination::new(options), faults)),
        )
    }

    fn place(
        &self,
        loader: &mut Loader<'_>,
        destination: &Destination,
        rest: Value<'_>,
        at: At<'_>,
        placing: Placing<'_>,
    ) -> Option<(Placed, &'static str, bool)> {
        let options = destination.of::<K::Options>()?;
        let route = loader.typed::<K::Route>(rest, at.path)?;
        let faults = K::placed(options, &route, placing);
        Some((Placed::new(route), K::LABEL, clean(loader, at, &faults)))
    }

    fn outlasts(&self, destination: &Destination, total: SignedDuration) -> Option<Fault> {
        K::outlasts(destination.of::<K::Options>()?, total)
    }

    fn destination_schema(&self, generator: &mut SchemaGenerator) -> Schema {
        tagged::<K::Options>(generator, K::NAME, K::DESCRIPTION)
    }

    fn route_schema(&self, generator: &mut SchemaGenerator) -> Schema {
        routed::<K::Route>(generator, K::ROUTE, K::ROUTE_DESCRIPTION)
    }
}

/// The destination `checked` admits, with every fault it was checked with
/// said where its table is written.
fn admitted(
    loader: &mut Loader<'_>,
    at: At<'_>,
    checked: Result<(Destination, Vec<Fault>), Fault>,
) -> Option<Destination> {
    let (destination, faults) = match checked {
        Ok((destination, faults)) => (Some(destination), faults),
        Err(fault) => (None, vec![fault]),
    };
    clean(loader, at, &faults);
    destination
}

/// Whether there are no `faults`, each said where its table is written.
fn clean(loader: &mut Loader<'_>, at: At<'_>, faults: &[Fault]) -> bool {
    for fault in faults {
        loader.fault(at, fault);
    }
    faults.is_empty()
}

/// One store kind, its types erased.
pub(crate) trait StoreKinded: Send + Sync {
    fn name(&self) -> &'static str;

    fn retained(&self) -> bool;

    fn parse(&self, loader: &mut Loader<'_>, rest: Value<'_>) -> Option<Store>;

    fn schema(&self, generator: &mut SchemaGenerator) -> Schema;
}

impl<K> StoreKinded for Of<K>
where
    K: StoreKind,
    K::Options: DeserializeOwned + JsonSchema,
{
    fn name(&self) -> &'static str {
        K::NAME
    }

    fn retained(&self) -> bool {
        K::RETAINED
    }

    fn parse(&self, loader: &mut Loader<'_>, rest: Value<'_>) -> Option<Store> {
        loader.typed::<K::Options>(rest, "store").map(Store::new)
    }

    fn schema(&self, generator: &mut SchemaGenerator) -> Schema {
        store::<K::Options>(generator, K::NAME, K::DESCRIPTION, K::RETAINED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_list_their_names_in_the_order_they_are_given() {
        let none = Kinds::new();
        assert_eq!(none.destination_kinds(), Vec::<&str>::new());
        assert_eq!(none.store_kinds(), Vec::<&str>::new());
        assert_eq!(
            format!("{none:?}"),
            "Kinds { destinations: [], stores: [] }"
        );
        assert!(none.destination_named("discord").is_none());
        assert!(none.store_named("sqlite").is_none());
    }
}
