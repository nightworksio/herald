use herald_config::{Config, Uses};
use herald_core::{Attribute, Draft, Event, Kind, SIGNAL, Signal, SourceName, Step};
use herald_flows::{Outcome, Ports, announce, prepare};
use herald_ports::Subscriptions;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};

use crate::activitypub::{activitypub, follower};
use crate::harness::*;

const EDITED: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages/1",
    200,
    r#"{"id":"1"}"#,
);
const RENAMED: (&str, u16, &str) = ("/channels/7", 200, r#"{"id":"7"}"#);
const INBOX: &str = "https://social.example/inbox";

/// The event of `v1` as `signal` announces it, its notes `body`, an edit
/// where `edited` says.
fn signalled(signal: Signal, body: &str, edited: bool) -> Event {
    let mut attributes = herald_core::versioned("v1".to_owned(), false);
    attributes.insert(SIGNAL.to_owned(), Attribute::Text(signal.name().to_owned()));
    let event = Event::normalise(Draft {
        identity: "github.com/1/v1".to_owned(),
        kind: Kind::RELEASE,
        project: "php-mutation-gate".to_owned(),
        attributes,
        title: "php-mutation-gate".to_owned(),
        body: body.to_owned(),
        links: [("release".to_owned(), "https://example.org/r".to_owned())].into(),
        occurred: NOON,
    })
    .unwrap();
    if edited { event.marked_edited() } else { event }
}

fn announced_with(
    config: &Config,
    (store, secrets): (&Failing, &FakeSecrets),
    script: &Script,
    event: &Event,
) -> Outcome {
    let prepared = pollster::block_on(prepare(config, secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let announced = pollster::block_on(announce(
        config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        event,
        "r1",
    ));
    outcome(&announced).clone()
}

fn announced(config: &Config, store: &Failing, script: &Script, event: &Event) -> Outcome {
    announced_with(config, (store, &secrets()), script, event)
}

fn posted() -> Script {
    Script::answering(&[POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS])
}

/// The steps an outcome landed, each edit and rename named by its kind.
fn landed(outcome: &Outcome) -> Vec<String> {
    let Outcome::Sent { steps } = outcome else {
        panic!("{outcome:?}");
    };
    steps
        .iter()
        .map(|step| match step {
            Step::Edit { part, .. } => format!("edit {part}"),
            Step::Rename(_) => "rename".to_owned(),
            Step::Deliver(inbox) => format!("deliver {inbox}"),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn a_release_after_its_tag_s_post_edits_the_post_and_renames_its_thread_once() {
    let (config, store) = (config(""), healthy());
    let tag = signalled(Signal::Tag, "", false);
    assert_eq!(
        announced(&config, &store, &posted(), &tag),
        Outcome::Sent { steps: all() }
    );
    let release = signalled(Signal::Release, "## Added", false);
    let script = Script::answering(&[EDITED, RENAMED]);
    assert_eq!(
        landed(&announced(&config, &store, &script, &release)),
        ["edit 0", "rename"]
    );
    let body: serde_json::Value = serde_json::from_slice(&script.bodies()[0]).unwrap();
    assert_eq!(body["embeds"][0]["description"], "## Added");
    assert_eq!(
        announced(&config, &store, &Script::default(), &release),
        Outcome::AlreadyAnnounced
    );
    assert_eq!(
        announced(&config, &store, &Script::default(), &tag),
        Outcome::AlreadyAnnounced
    );
}

#[test]
fn an_edit_that_stops_takes_up_its_remaining_steps_on_the_next_run() {
    let (config, store) = (config(""), healthy());
    announced(
        &config,
        &store,
        &posted(),
        &signalled(Signal::Tag, "", false),
    );
    let release = signalled(Signal::Release, "## Added", false);
    let refused = Script::answering(&[EDITED, ("/channels/7", 403, r#"{"code":50013}"#)]);
    assert!(matches!(
        announced(&config, &store, &refused, &release),
        Outcome::Stopped { remaining, .. } if matches!(remaining[..], [Step::Rename(_)])
    ));
    assert_eq!(
        landed(&announced(
            &config,
            &store,
            &Script::answering(&[RENAMED]),
            &release
        )),
        ["rename"]
    );
}

#[test]
fn a_late_release_is_posted_anew_or_left_as_the_route_says() {
    let tag = signalled(Signal::Tag, "", false);
    let release = signalled(Signal::Release, "## Added", false);
    let reposting = config("late_release = \"post\"");
    let store = healthy();
    announced(&reposting, &store, &posted(), &tag);
    assert_eq!(
        announced(&reposting, &store, &posted(), &release),
        Outcome::Sent { steps: all() }
    );
    assert_eq!(
        announced(&reposting, &store, &Script::default(), &release),
        Outcome::AlreadyAnnounced
    );
    let skipping = config("late_release = \"skip\"");
    let store = healthy();
    announced(&skipping, &store, &posted(), &tag);
    assert_eq!(
        announced(&skipping, &store, &Script::default(), &release),
        Outcome::AlreadyAnnounced
    );
}

#[test]
fn an_edited_release_edits_its_post_only_where_the_route_takes_edits_and_never_posts() {
    let release = signalled(Signal::Release, "## Added", false);
    let edited = signalled(Signal::Release, "## Added\n- a fix", true);
    let taking = config("edits = \"release\"");
    let store = healthy();
    assert_eq!(
        announced(&taking, &store, &Script::default(), &edited),
        Outcome::Unedited
    );
    announced(&taking, &store, &posted(), &release);
    assert_eq!(
        landed(&announced(
            &taking,
            &store,
            &Script::answering(&[EDITED, RENAMED]),
            &edited
        )),
        ["edit 0", "rename"]
    );
    let store = healthy();
    announced(&config(""), &store, &posted(), &release);
    assert_eq!(
        announced(&config(""), &store, &Script::default(), &edited),
        Outcome::AlreadyAnnounced
    );
}

#[test]
fn a_note_s_update_reaches_each_follower_its_create_reached_and_no_other() {
    let (config, mut store) = (activitypub(), healthy());
    let secrets = FakeSecrets::default().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    pollster::block_on(store.subscribe(&follower(
        "php-mutation-gate",
        "https://social.example/users/ana",
        Some(INBOX),
    )))
    .unwrap();
    let tag = signalled(Signal::Tag, "", false);
    let created = Script::answering(&[(INBOX, 202, "")]);
    assert_eq!(
        landed(&announced_with(&config, (&store, &secrets), &created, &tag)),
        ["Post", format!("deliver {INBOX}").as_str()]
    );
    pollster::block_on(store.subscribe(&follower(
        "php-mutation-gate",
        "https://late.example/users/bo",
        Some("https://late.example/inbox"),
    )))
    .unwrap();
    let release = signalled(Signal::Release, "## Added", false);
    store.fault = Fault::ReadsDeliveries;
    assert!(matches!(
        announced_with(&config, (&store, &secrets), &Script::default(), &release),
        Outcome::Stopped { remaining, .. } if matches!(remaining[..], [Step::Updates(_)])
    ));
    store.fault = Fault::Nothing;
    let updated = Script::answering(&[(INBOX, 202, "")]);
    assert_eq!(
        landed(&announced_with(
            &config,
            (&store, &secrets),
            &updated,
            &release
        )),
        [format!("deliver {INBOX}").as_str()]
    );
    let update: serde_json::Value = serde_json::from_slice(&updated.bodies()[0]).unwrap();
    assert_eq!(update["type"], "Update");
    assert_eq!(
        announced_with(&config, (&store, &secrets), &Script::default(), &release),
        Outcome::AlreadyAnnounced
    );
}

#[test]
fn a_post_the_route_keeps_still_shows_what_it_first_announced_when_more_is_posted() {
    let release = signalled(Signal::Release, "## Added", false);
    let longer = signalled(Signal::Release, &"word ".repeat(1200), true);
    let store = healthy();
    announced(&config(""), &store, &posted(), &release);
    let chunked = Script::answering(&[("/channels/7/messages", 200, r#"{"id":"2"}"#)]);
    assert!(matches!(
        announced(&config(""), &store, &chunked, &longer),
        Outcome::Sent { steps } if matches!(steps[..], [Step::Chunk(1)])
    ));
    let edits = Script::answering(&[
        EDITED,
        ("/channels/7/messages/2", 200, r#"{"id":"2"}"#),
        RENAMED,
    ]);
    assert_eq!(
        landed(&announced(
            &config("edits = \"release\""),
            &store,
            &edits,
            &longer
        )),
        ["edit 0", "edit 1", "rename"]
    );
}
