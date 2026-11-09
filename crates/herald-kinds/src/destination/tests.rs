use std::any::Any;

use herald_core::{Profile, SecretRef, Symbols, WaitOptions, Waits};
use herald_ports::Secret;
use jiff::SignedDuration;

use super::*;
use crate::reach::tests::{PROFILE, Plain as Reaching};
use crate::{Placed, Placement, Reached, same};

/// Options of a kind that keeps every default.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plain {
    key: SecretRef,
    symbols: Option<Symbols>,
    waits: Option<WaitOptions>,
}

impl DestinationConfig for Plain {
    fn kind(&self) -> &'static str {
        "plain"
    }

    fn symbols_written(&self) -> Option<Symbols> {
        self.symbols
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn credential(&self) -> Option<&SecretRef> {
        Some(&self.key)
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        vec![&self.key]
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        placed.of::<Spot>().map(|_| (PROFILE, Some(PROFILE)))
    }

    fn reach(&self, placed: &Placed, base: &str, _: Secret) -> Option<Reached> {
        placed.of::<Spot>()?;
        Some(Reached::new(Reaching(if base.is_empty() {
            "https://plain.example"
        } else {
            "https://based.example"
        })))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

/// Options of another kind, whose platform waits longer.
#[derive(Debug, PartialEq, Eq)]
struct Patient;

impl Patient {
    const WAITS: Waits = Waits {
        outage: SignedDuration::from_mins(1),
        limited: SignedDuration::from_mins(2),
    };
}

impl DestinationConfig for Patient {
    fn kind(&self) -> &'static str {
        "patient"
    }

    fn symbols_written(&self) -> Option<Symbols> {
        None
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        None
    }

    fn platform_waits(&self) -> Waits {
        Self::WAITS
    }

    fn credential(&self) -> Option<&SecretRef> {
        None
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        Vec::new()
    }

    fn profile(&self, _: &Placed) -> Option<(Profile, Option<Profile>)> {
        None
    }

    fn reach(&self, _: &Placed, _: &str, _: Secret) -> Option<Reached> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

/// A route's options of the plain kind, which keep every default.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Spot(&'static str);

impl Placement for Spot {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

/// A route's options of another kind, which names its place.
#[derive(Debug, PartialEq, Eq)]
struct Room;

impl Placement for Room {
    fn threads_each_post(&self) -> bool {
        true
    }

    fn place(&self) -> Option<&str> {
        Some("!room")
    }

    fn opt_in_role(&self) -> Option<String> {
        Some("role".to_owned())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

fn plain(secret: &str) -> Plain {
    Plain {
        key: SecretRef {
            secret: secret.to_owned(),
        },
        symbols: None,
        waits: None,
    }
}

#[test]
fn a_destination_is_its_kind_equal_only_to_the_same_options_and_keeps_every_default() {
    let destination = Destination::new(plain("HERALD_KEY"));
    assert_eq!(destination, destination.clone());
    assert_eq!(destination, Destination::new(plain("HERALD_KEY")));
    assert_ne!(destination, Destination::new(plain("HERALD_OTHER")));
    assert_ne!(destination, Destination::new(Patient));
    assert_eq!(destination.of::<Plain>(), Some(&plain("HERALD_KEY")));
    assert_eq!(destination.of::<Patient>(), None);
    assert_eq!(
        format!("{destination:?}"),
        "Plain { key: SecretRef { secret: \"HERALD_KEY\" }, symbols: None, waits: None }"
    );
    assert_eq!(destination.kind(), "plain");
    assert_eq!(
        destination.credential().map(|named| named.secret.as_str()),
        Some("HERALD_KEY")
    );
    assert_eq!(destination.secret_refs().len(), 1);
    assert_eq!(destination.answering(), None);
    assert_eq!(destination.carried(), None);
    assert!(!destination.acts());
    assert_eq!(destination.session_domains(), Vec::<&str>::new());
    let patient = Destination::new(Patient);
    assert_eq!(patient.kind(), "patient");
    assert_eq!(patient.credential(), None);
    assert_eq!(patient.secret_refs(), Vec::<&SecretRef>::new());
}

#[test]
fn a_destination_writes_its_symbols_and_waits_else_the_defaults_and_its_platforms() {
    let unset = Destination::new(plain("HERALD_KEY"));
    assert_eq!(unset.symbols(), Symbols::default());
    assert_eq!(unset.waits(), Waits::DEFAULT);
    assert_eq!(Destination::new(Patient).waits(), Patient::WAITS);
    assert_eq!(Destination::new(Patient).symbols(), Symbols::default());
    let written: WaitOptions = serde_json::from_str(r#"{ "outage": "7s" }"#).unwrap();
    let symbols: Symbols = serde_json::from_str(r#"{ "bullet": "*" }"#).unwrap();
    let set = Destination::new(Plain {
        symbols: Some(symbols),
        waits: Some(written),
        ..plain("HERALD_KEY")
    });
    assert_eq!(set.symbols(), symbols);
    assert_eq!(set.waits(), Waits::of(Some(written), Waits::DEFAULT));
    assert_eq!(set.waits().outage, SignedDuration::from_secs(7));
}

#[test]
fn a_route_is_placed_as_its_kind_and_keeps_every_default() {
    let spot = Placed::new(Spot("here"));
    assert_eq!(spot, spot.clone());
    assert_ne!(spot, Placed::new(Spot("there")));
    assert_ne!(spot, Placed::new(Room));
    assert_eq!(spot.of::<Spot>(), Some(&Spot("here")));
    assert_eq!(spot.of::<Room>(), None);
    assert_eq!(format!("{spot:?}"), "Spot(\"here\")");
    assert!(!spot.threads_each_post());
    assert_eq!(spot.place(), None);
    assert_eq!(spot.opt_in_role(), None);
    let room = Placed::new(Room);
    assert!(room.threads_each_post());
    assert_eq!(room.place(), Some("!room"));
    assert_eq!(room.opt_in_role().as_deref(), Some("role"));
}

#[test]
fn a_destination_reaches_and_profiles_only_routes_placed_as_its_kind() {
    let destination = Destination::new(plain("HERALD_KEY"));
    let spot = Placed::new(Spot("here"));
    let secret = || Secret::new("HERALD_KEY", "key");
    assert_eq!(destination.profile(&spot), Some((PROFILE, Some(PROFILE))));
    assert_eq!(destination.profile(&Placed::new(Room)), None);
    assert_eq!(
        destination.reach(&spot, "", secret()),
        Some(Reached::new(Reaching("https://plain.example")))
    );
    assert_eq!(
        destination.reach(&spot, "https://herald.example", secret()),
        Some(Reached::new(Reaching("https://based.example")))
    );
    assert_eq!(destination.reach(&Placed::new(Room), "", secret()), None);
    let patient = Destination::new(Patient);
    assert_eq!(patient.profile(&spot), None);
    assert_eq!(patient.reach(&spot, "", secret()), None);
}
