use std::num::NonZeroUsize;

use herald_core::{Chore, Entry, Sent, Size, Step};
use herald_kinds::{Bound, DestinationConfig, DestinationKind, Fault, Feeding, Placed, Placing};
use herald_ports::{Base, Secret};
use herald_testing::{FakeSigner, ScriptedHttp, sample};

use super::*;

fn options(text: &str) -> FeedOptions {
    toml::from_str(text).unwrap()
}

fn checked(text: &str) -> Result<FeedOptions, Fault> {
    HeraldKind::checked(options(text), &|_| Err(String::new())).map(|(options, faults)| {
        assert_eq!(faults, Vec::new());
        options
    })
}

fn feeding(options: &FeedOptions) -> Feeding {
    options.feeding().unwrap()
}

#[test]
fn a_feed_keeps_lists_and_bounds_its_envelopes_by_default() {
    let unset = checked("").unwrap();
    assert_eq!(
        feeding(&unset),
        Feeding {
            keep: 1000,
            page: 20,
            envelope: 16 * 1024,
            bound: Bound::Default,
        }
    );
    let set = checked("keep = 4000\npage = 80\nenvelope = \"64KiB\"").unwrap();
    assert_eq!(
        feeding(&set),
        Feeding {
            keep: 4000,
            page: 80,
            envelope: 64 * 1024,
            bound: Bound::Named,
        }
    );
}

#[test]
fn each_option_past_its_bound_is_refused() {
    for (text, key, message) in [
        ("keep = 4001", "keep", "keep is at most 4000; lower it"),
        ("page = 81", "page", "page is at most 80; lower it"),
        (
            "envelope = \"65KiB\"",
            "envelope",
            "envelope is at most \"64KiB\"; lower it",
        ),
    ] {
        assert_eq!(checked(text), Err(Fault::at(key, message)), "{text}");
    }
    assert!(toml::from_str::<FeedOptions>("domain = \"x\"").is_err());
}

#[test]
fn a_feed_naming_no_bound_is_fitted_to_its_store() {
    let unset = checked("page = 5").unwrap();
    let fitted = unset.fitted(3).unwrap();
    let fitted = fitted.of::<FeedOptions>().unwrap();
    assert_eq!(
        feeding(fitted),
        Feeding {
            keep: 3,
            page: 5,
            envelope: FeedOptions::ENVELOPE,
            bound: Bound::Fitted,
        }
    );
    assert_eq!(checked("keep = 9").unwrap().fitted(3).map(|_| ()), None);
    let named = FeedOptions {
        keep: NonZeroUsize::new(9),
        fitted: Some(3),
        ..FeedOptions::default()
    };
    assert_eq!(feeding(&named).keep, 9);
}

#[test]
fn a_feed_signs_with_the_instance_s_key_and_names_no_secret_of_its_own() {
    let unset = checked("").unwrap();
    assert_eq!(unset.kind(), HERALD);
    assert_eq!(unset.credential(), None);
    assert_eq!(unset.secret_refs(), Vec::<&herald_core::SecretRef>::new());
    assert_eq!(unset.symbols_written(), None);
    assert_eq!(unset.waits_written(), None);
    assert!(unset.unread_why().contains("serves its envelopes"));
    let placed = Placed::new(FeedRouteOptions::default());
    let (profile, bot) = unset.profile(&placed).unwrap();
    assert_eq!(bot, None);
    assert!(profile.features.edits && !profile.features.reads_back);
    assert_eq!(
        HeraldKind::placed(
            &unset,
            &FeedRouteOptions::default(),
            Placing {
                destination: "feed",
                subscribers: false,
            }
        ),
        Vec::new()
    );
    let other = Placed::new(Elsewhere);
    assert!(unset.profile(&other).is_none());
    assert!(unset.reach(&other, "", key()).is_none());
}

#[derive(Debug, PartialEq)]
struct Elsewhere;

impl herald_kinds::Placement for Elsewhere {}

fn key() -> Secret {
    Secret::new("HERALD_FEDERATION_KEY", "k")
}

#[test]
fn a_reached_feed_is_published_to_by_the_flows_and_sent_nothing() {
    let unset = checked("").unwrap();
    let reached = unset
        .reach(&Placed::new(FeedRouteOptions::default()), "", key())
        .unwrap();
    let fed = reached.feeds().unwrap();
    assert_eq!(fed.feeding, feeding(&unset));
    assert_eq!(fed.key, &key());
    assert_eq!(
        reached.profile(),
        unset
            .profile(&Placed::new(FeedRouteOptions::default()))
            .unwrap()
            .0
    );
    assert_eq!(reached.base().bases(), Some(Vec::new()));
    assert!(matches!(reached.base(), Base::Among([])));
    let http = ScriptedHttp::new(Vec::new());
    let signer = FakeSigner;
    let signing = herald_kinds::Signing {
        signer: &signer,
        now: jiff::Timestamp::UNIX_EPOCH,
    };
    let announcement = sample::announcement();
    let entry = Entry::new(announcement.key);
    let sent = pollster::block_on(reached.send(
        &http,
        signing,
        &Step::Post,
        &announcement,
        &sample::delivery("role"),
        &entry,
    ));
    let refused = |sent: Sent| matches!(sent, Sent::Refused(why) if why.contains("published to by herald itself"));
    assert!(refused(sent.unwrap()));
    let kept = pollster::block_on(reached.housekeep(&http, Chore::Archive, "t"));
    assert!(refused(kept.unwrap()));
    assert_eq!(http.asked(), Vec::new());
    let read = pollster::block_on(herald_ports::Markers::threads(
        &reached.read(&http),
        &sample::route_marker(),
    ));
    assert!(
        read.unwrap_err()
            .to_string()
            .contains("serves its envelopes")
    );
    assert_eq!(Size::KIB, 1024);
}
