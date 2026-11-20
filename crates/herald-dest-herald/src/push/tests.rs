use herald_core::{Chore, Entry, Sent, Step, Waits};
use herald_kinds::{
    DestinationConfig, DestinationKind, Fault, IdTokens, Peered, Placed, Placement, Placing,
    Proving, Pushing,
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
            inbox: "releases",
            id_token: false,
            audience: None,
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
    for (written, key) in [
        ("id_token = \"variables\"", "id_token"),
        ("id_tokens = [{ secret = \"T\" }]", "id_tokens"),
        ("audience = \"https://peer.example\"", "audience"),
    ] {
        assert_eq!(
            checked(&format!("{NAMED}\n{written}")),
            Err(Fault::at(
                key,
                format!(
                    "{key} is read only with credential = \"id-token\"; set it, or leave {key} out"
                )
            )),
            "{written}"
        );
    }
    let keyed = checked(&format!("{NAMED}\ncredential = \"key\"")).unwrap();
    assert_eq!(
        (keyed.credential(), keyed.pushing(), keyed.reads()),
        (None, named.pushing(), Vec::new())
    );
}

fn peered(options: &PushOptions) -> Destination {
    options
        .peered(Peered {
            url: "https://peer.example".to_owned(),
            private: false,
            presented: herald_kinds::Presented::default(),
        })
        .unwrap()
}

#[test]
fn a_push_on_github_actions_asks_for_a_fresh_id_token_for_each_push() {
    let defaulted = checked(&format!("{NAMED}\ncredential = \"id-token\"")).unwrap();
    let named = |name: &str| SecretRef {
        secret: name.to_owned(),
    };
    let (token, url) = (
        named(PushOptions::ACTIONS_TOKEN),
        named(PushOptions::ACTIONS_URL),
    );
    assert_eq!(defaulted.credential(), Some(&token));
    assert_eq!(defaulted.reads(), vec![&url]);
    assert_eq!(defaulted.secret_refs(), vec![&token, &url]);
    assert_eq!(
        defaulted.pushing(),
        Some(Pushing {
            peer: "upstream",
            inbox: "releases",
            id_token: true,
            audience: None,
        })
    );
    let asking = Secret::new(PushOptions::ACTIONS_TOKEN, "t");
    let at = Secret::new(
        PushOptions::ACTIONS_URL,
        "https://pipelines.actions.example/idtoken?api-version=2.0",
    );
    let reached = peered(&defaulted)
        .reach_reading(&placed(), "", (asking.clone(), vec![at.clone()]))
        .unwrap();
    assert!(matches!(
        reached.pushes().unwrap().proving,
        Proving::IdToken {
            tokens: IdTokens::Requested { url, token },
            audience: "https://peer.example",
        } if *url == at && *token == asking
    ));
    assert_eq!(
        reached.base(),
        Base::Among(&[
            "https://peer.example".to_owned(),
            "https://pipelines.actions.example".to_owned()
        ])
    );
    let malformed = peered(&defaulted)
        .reach_reading(
            &placed(),
            "",
            (
                asking.clone(),
                vec![Secret::new(PushOptions::ACTIONS_URL, "not a url")],
            ),
        )
        .unwrap();
    assert_eq!(malformed.base(), Base::At("https://peer.example"));
    assert!(
        peered(&defaulted)
            .reach(&placed(), "", asking.clone())
            .is_none()
    );
    assert_eq!(
        checked(&format!(
            "{NAMED}\ncredential = \"id-token\"\nid_tokens = [{{ secret = \"T\" }}]"
        )),
        Err(Fault::at(
            "id_tokens",
            "id_tokens are read only with id_token = \"variables\"; set it, or leave id_tokens out"
        ))
    );
    let aimed = checked(&format!(
        "{NAMED}\ncredential = \"id-token\"\naudience = \"https://peer.example/ci\""
    ))
    .unwrap();
    assert_eq!(
        aimed.pushing().unwrap().audience,
        Some("https://peer.example/ci")
    );
    let reached = peered(&aimed)
        .reach_reading(&placed(), "", (asking, vec![at]))
        .unwrap();
    assert!(matches!(
        reached.pushes().unwrap().proving,
        Proving::IdToken {
            audience: "https://peer.example/ci",
            ..
        }
    ));
    assert_eq!(
        checked(&format!(
            "{NAMED}\ncredential = \"id-token\"\naudience = \"\""
        )),
        Err(Fault::at(
            "audience",
            "the audience is empty; give the URL the peer's tokens name, or leave audience out"
        ))
    );
    assert!(toml::from_str::<PushOptions>(&format!("{NAMED}\ncredential = \"none\"")).is_err());
    assert!(toml::from_str::<PushOptions>(&format!("{NAMED}\nid_token = \"env\"")).is_err());
}

#[test]
fn a_push_on_another_ci_presents_each_id_token_its_variables_hold_once_in_turn() {
    let variables = "credential = \"id-token\"\nid_token = \"variables\"";
    let defaulted = checked(&format!("{NAMED}\n{variables}")).unwrap();
    let read = SecretRef {
        secret: PushOptions::ID_TOKEN.to_owned(),
    };
    assert_eq!(defaulted.credential(), Some(&read));
    assert_eq!(defaulted.reads(), Vec::<&SecretRef>::new());
    let several = checked(&format!(
        "{NAMED}\n{variables}\nid_tokens = [{{ secret = \"FIRST\" }}, {{ secret = \"SECOND\" }}]"
    ))
    .unwrap();
    assert_eq!(
        several.credential().map(|named| named.secret.as_str()),
        Some("FIRST")
    );
    assert_eq!(
        several
            .reads()
            .iter()
            .map(|named| named.secret.as_str())
            .collect::<Vec<_>>(),
        ["SECOND"]
    );
    let (first, second) = (Secret::new("FIRST", "a"), Secret::new("SECOND", "b"));
    let destination = peered(&several);
    let one = destination
        .reach_reading(&placed(), "", (first.clone(), vec![second.clone()]))
        .unwrap();
    let other = destination
        .reach_reading(&placed(), "", (first.clone(), vec![second.clone()]))
        .unwrap();
    assert_eq!(one.base(), Base::At("https://peer.example"));
    let next = |reached: &Reached| match reached.pushes().unwrap().proving {
        Proving::IdToken {
            tokens: IdTokens::Read { tokens, presented },
            ..
        } => presented.next(tokens),
        Proving::Key(_) | Proving::IdToken { .. } => panic!("read from the run's variables"),
    };
    assert_eq!(next(&one), Some(first));
    assert_eq!(next(&other), Some(second));
    assert_eq!(next(&one), None);
    assert_eq!(
        checked(&format!("{NAMED}\n{variables}\nid_tokens = []")),
        Err(Fault::at(
            "id_tokens",
            "no ID token is read, so nothing is pushed; give one variable for each push, or leave id_tokens out"
        ))
    );
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
            presented: herald_kinds::Presented::default(),
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
            presented: herald_kinds::Presented::default(),
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
    assert!(matches!(pushed.proving, Proving::Key(proving) if *proving == key()));
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
