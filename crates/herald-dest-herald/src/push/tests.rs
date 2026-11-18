use herald_core::{Chore, Entry, Sent, Step, Waits};
use herald_kinds::{
    DestinationConfig, DestinationKind, Fault, Peered, Placed, Placement, Placing, Pushing,
};
use herald_ports::{Base, Secret};
use herald_testing::{FakeSigner, ScriptedHttp, sample};

use super::*;

fn checked(text: &str) -> Result<PushOptions, Fault> {
    PushKind::checked(toml::from_str(text).unwrap(), &|_| Err(String::new())).map(
        |(options, faults)| {
            assert_eq!(faults, Vec::new());
            options
        },
    )
}

const NAMED: &str = "peer = \"upstream\"\ninbox = \"releases\"";

fn key() -> Secret {
    Secret::new("HERALD_FEDERATION_KEY", "k")
}

fn placed() -> Placed {
    Placed::new(PushRouteOptions::default())
}

#[derive(Debug, PartialEq)]
struct Elsewhere;

impl Placement for Elsewhere {}

#[test]
fn a_push_names_a_peer_and_its_source_and_bounds_its_envelopes() {
    let named = checked(NAMED).unwrap();
    assert_eq!(
        named.pushing(),
        Some(Pushing {
            peer: "upstream",
            inbox: "releases"
        })
    );
    assert_eq!(named.envelope(), PushOptions::ENVELOPE);
    assert_eq!(
        checked(&format!("{NAMED}\nenvelope = \"64KiB\""))
            .unwrap()
            .envelope(),
        64 * 1024
    );
    for (text, key, message) in [
        (
            "peer = \"Upstream\"\ninbox = \"releases\"",
            "peer",
            "\"Upstream\" is not a peer's name; name a table of [peers]",
        ),
        (
            "peer = \"upstream\"\ninbox = \"..\"",
            "inbox",
            "\"..\" is not a source's name a URL carries as it is; give the name of the peer's herald source, of letters, digits, '-', '_' and '.'",
        ),
        (
            &format!("{NAMED}\nenvelope = \"65KiB\""),
            "envelope",
            "envelope is at most \"64KiB\"; lower it",
        ),
    ] {
        assert_eq!(checked(text), Err(Fault::at(key, message)), "{text}");
    }
    for inbox in ["", ".", "a/b", "a?b", "a b", "%2e"] {
        assert!(!segment(inbox), "{inbox}");
    }
    for inbox in ["releases", "Mirror.v1", "a_b-c"] {
        assert!(segment(inbox), "{inbox}");
    }
    assert!(toml::from_str::<PushOptions>("peer = \"upstream\"").is_err());
}

#[test]
fn a_push_signs_with_the_instance_s_key_and_waits_as_its_config_says() {
    let named = checked(&format!("{NAMED}\nwaits = {{ outage = \"40s\" }}")).unwrap();
    assert_eq!(named.kind(), HERALD_PUSH);
    assert_eq!(named.credential(), None);
    assert_eq!(named.secret_refs(), Vec::<&SecretRef>::new());
    assert_eq!(named.symbols_written(), None);
    assert_eq!(named.waits().outage, jiff::SignedDuration::from_secs(40));
    assert!(named.unread_why().contains("pushes its envelopes"));
    let (profile, bot) = named.profile(&placed()).unwrap();
    assert_eq!(bot, None);
    assert!(profile.features.edits && !profile.features.reads_back);
    assert_eq!(
        PushKind::placed(
            &named,
            &PushRouteOptions::default(),
            Placing {
                destination: "mirror",
                subscribers: false,
            }
        ),
        Vec::new()
    );
    let other = Placed::new(Elsewhere);
    assert!(named.profile(&other).is_none());
    assert!(named.reach(&placed(), "", key()).is_none());
    let peered = named
        .peered(Peered {
            url: "https://peer.example/h".to_owned(),
            private: true,
        })
        .unwrap();
    assert!(peered.reach(&other, "", key()).is_none());
}

#[test]
fn a_reached_push_goes_to_the_peer_s_inbox_and_is_sent_nothing_step_by_step() {
    let named = checked(NAMED).unwrap();
    let peered = named
        .peered(Peered {
            url: "https://peer.example/h".to_owned(),
            private: false,
        })
        .unwrap();
    let reached = peered.reach(&placed(), "", key()).unwrap();
    let pushed = reached.pushes().unwrap();
    assert_eq!(pushed.peer, "upstream");
    assert_eq!(
        pushed.inbox,
        "https://peer.example/h/herald/v1/inbox/releases"
    );
    assert_eq!(pushed.envelope, PushOptions::ENVELOPE);
    assert!(!pushed.private);
    assert_eq!(pushed.waits, Waits::DEFAULT);
    assert_eq!(pushed.key, &key());
    assert!(reached.feeds().is_none());
    assert_eq!(reached.profile(), named.profile(&placed()).unwrap().0);
    assert_eq!(reached.base(), Base::At("https://peer.example/h"));
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
    let refused = |sent: Sent| matches!(sent, Sent::Refused(why) if why.contains("pushed to by herald itself"));
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
            .contains("pushes its envelopes")
    );
}
