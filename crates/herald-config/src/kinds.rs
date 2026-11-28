use std::fmt;
use std::marker::PhantomData;
use std::ops::Range;

use herald_kinds::{
    ConfiguredStore, Destination, DestinationKind, Fault, FirstRun, Inherited, Offer, Placed,
    Placing, Source, SourceKind, StoreKind,
};
use jiff::SignedDuration;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::DeserializeOwned;

use crate::load::{Loader, Value};
use crate::schema::{routed, store, tagged};

/// Every kind of destination, source and store a herald reads its config
/// with: the one list each host composes, which the loader parses through
/// and the schema is generated from.
#[derive(Default)]
pub struct Kinds {
    destinations: Vec<Box<dyn Kinded>>,
    sources: Vec<Box<dyn SourceKinded>>,
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

    /// Every destination kind a first run may choose, in the order they
    /// are given: each that names a way it is first reached.
    #[must_use]
    pub fn first_runs(&self) -> Vec<FirstRun> {
        self.destinations
            .iter()
            .filter_map(|kind| kind.first_run())
            .collect()
    }

    pub(crate) fn destination_named(&self, name: &str) -> Option<&dyn Kinded> {
        self.destinations
            .iter()
            .map(AsRef::as_ref)
            .find(|kind| kind.name() == name)
    }

    /// The name of every destination kind whose routes render a template,
    /// each the dialect it converts to.
    pub(crate) fn dialects(&self) -> Vec<&'static str> {
        self.destinations
            .iter()
            .filter(|kind| kind.renders())
            .map(|kind| kind.name())
            .collect()
    }

    /// Each destination kind's name, every key its own route options take,
    /// and those a local preset may write, says where a post goes, and may
    /// not write (ADR-0092 decision 4).
    #[must_use]
    pub fn preset_route_options(&self) -> Vec<PresetOptions> {
        self.destinations
            .iter()
            .map(|kind| PresetOptions {
                kind: kind.name(),
                taken: kind.route_keys(),
                writes: kind.preset_writes(),
                addressing: kind.addressing(),
                refuses: kind.preset_refuses(),
            })
            .collect()
    }

    pub(crate) fn destinations(&self) -> impl Iterator<Item = &dyn Kinded> {
        self.destinations.iter().map(AsRef::as_ref)
    }

    /// These kinds, and sources of kind `K`.
    #[must_use]
    pub fn source<K>(mut self) -> Self
    where
        K: SourceKind,
        K::File: DeserializeOwned + JsonSchema,
    {
        self.sources.push(Box::new(Of::<K>(PhantomData)));
        self
    }

    /// Every source kind's name, as `kind` writes it.
    #[must_use]
    pub fn source_kinds(&self) -> Vec<&'static str> {
        self.sources.iter().map(|kind| kind.name()).collect()
    }

    pub(crate) fn source_named(&self, name: &str) -> Option<&dyn SourceKinded> {
        self.sources
            .iter()
            .map(AsRef::as_ref)
            .find(|kind| kind.name() == name)
    }

    pub(crate) fn sources(&self) -> impl Iterator<Item = &dyn SourceKinded> {
        self.sources.iter().map(AsRef::as_ref)
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

    /// The name of every store kind whose needs a host meets where it
    /// offers what `offers` holds for, in the order they are given.
    #[must_use]
    pub fn store_kinds_offered(&self, offers: impl Fn(Offer) -> bool) -> Vec<&'static str> {
        self.stores
            .iter()
            .filter(|kind| offers(kind.needs()))
            .map(|kind| kind.name())
            .collect()
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
            .field("sources", &self.source_kinds())
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

    fn inherits(&self) -> &'static [Inherited];

    fn first_run(&self) -> Option<FirstRun>;

    fn renders(&self) -> bool;

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

    fn route_keys(&self) -> Vec<String>;

    fn preset_writes(&self) -> &'static [&'static str];

    fn addressing(&self) -> &'static [&'static str];

    fn preset_refuses(&self) -> &'static [&'static str];
}

/// What a destination kind's own route options are to a local preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetOptions {
    /// The kind's name.
    pub kind: &'static str,
    /// Every key its own route options take.
    pub taken: Vec<String>,
    /// Those a local preset may write.
    pub writes: &'static [&'static str],
    /// Of those, the ones that say where a post goes or who sees it.
    pub addressing: &'static [&'static str],
    /// Those a local preset may not write.
    pub refuses: &'static [&'static str],
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

    fn inherits(&self) -> &'static [Inherited] {
        K::INHERITS
    }

    fn first_run(&self) -> Option<FirstRun> {
        (!K::FIRST_RUN.is_empty()).then_some(FirstRun {
            kind: K::NAME,
            described: K::DESCRIPTION,
            variants: K::FIRST_RUN,
        })
    }

    fn renders(&self) -> bool {
        K::RENDERS
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
        uninherited(
            tagged::<K::Options>(generator, K::NAME, K::DESCRIPTION),
            K::INHERITS,
        )
    }

    fn route_schema(&self, generator: &mut SchemaGenerator) -> Schema {
        routed::<K::Route>(generator, K::ROUTE, K::ROUTE_DESCRIPTION)
    }

    fn preset_writes(&self) -> &'static [&'static str] {
        K::PRESET_WRITES
    }

    fn addressing(&self) -> &'static [&'static str] {
        K::ADDRESSING
    }

    fn preset_refuses(&self) -> &'static [&'static str] {
        K::PRESET_REFUSES
    }

    fn route_keys(&self) -> Vec<String> {
        let schema = SchemaGenerator::default().into_root_schema_for::<K::Route>();
        keys_of(schema.as_value(), schema.as_value())
    }
}

/// The keys a table `schema` describes takes, each alternative's and each
/// flattened part's among them, its references read in `root`.
fn keys_of(schema: &serde_json::Value, root: &serde_json::Value) -> Vec<String> {
    let referenced = schema
        .get("$ref")
        .and_then(serde_json::Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/"))
        .and_then(|path| root.pointer(&format!("/{path}")));
    let mut keys: Vec<String> = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|properties| properties.keys().cloned())
        .chain(referenced.into_iter().flat_map(|part| keys_of(part, root)))
        .collect();
    for combined in ["allOf", "anyOf", "oneOf"] {
        for part in schema
            .get(combined)
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            keys.extend(keys_of(part, root));
        }
    }
    keys
}

/// `schema` with every key of `inherits` left out of what it requires, as
/// `[federation]` may give it.
fn uninherited(mut schema: Schema, inherits: &[Inherited]) -> Schema {
    schema
        .get_mut("required")
        .and_then(serde_json::Value::as_array_mut)
        .into_iter()
        .for_each(|required| {
            required.retain(|key| {
                !inherits
                    .iter()
                    .any(|inheriting| key.as_str() == Some(inheriting.key))
            });
        });
    schema
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

/// One source kind, its types erased.
pub(crate) trait SourceKinded: Send + Sync {
    fn name(&self) -> &'static str;

    fn parse(
        &self,
        loader: &mut Loader<'_>,
        rest: Value<'_>,
        at: At<'_>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Option<Source>;

    fn schema(&self, generator: &mut SchemaGenerator) -> Schema;
}

impl<K> SourceKinded for Of<K>
where
    K: SourceKind,
    K::File: DeserializeOwned + JsonSchema,
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
    ) -> Option<Source> {
        let file = loader.typed::<K::File>(rest, at.path)?;
        match K::checked(file).and_then(|options| K::read(options, read)) {
            Ok(options) => Some(Source::new(options)),
            Err(fault) => {
                clean(loader, at, &[fault]);
                None
            }
        }
    }

    fn schema(&self, generator: &mut SchemaGenerator) -> Schema {
        tagged::<K::File>(generator, K::NAME, K::DESCRIPTION)
    }
}

/// One store kind, its types erased.
pub(crate) trait StoreKinded: Send + Sync {
    fn name(&self) -> &'static str;

    fn needs(&self) -> Offer;

    fn retained(&self) -> bool;

    fn parse(&self, loader: &mut Loader<'_>, rest: Value<'_>) -> Option<ConfiguredStore>;

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

    fn needs(&self) -> Offer {
        K::NEEDS
    }

    fn retained(&self) -> bool {
        K::RETAINED
    }

    fn parse(&self, loader: &mut Loader<'_>, rest: Value<'_>) -> Option<ConfiguredStore> {
        loader
            .typed::<K::Options>(rest, "store")
            .map(ConfiguredStore::new)
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
        assert_eq!(none.source_kinds(), Vec::<&str>::new());
        assert_eq!(none.store_kinds(), Vec::<&str>::new());
        assert_eq!(
            format!("{none:?}"),
            "Kinds { destinations: [], sources: [], stores: [] }"
        );
        assert!(none.source_named("github").is_none());
        assert!(none.destination_named("discord").is_none());
        assert!(none.store_named("sqlite").is_none());
    }
}
