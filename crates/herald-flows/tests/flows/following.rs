use herald_core::{DestinationName, Facts, Sent};
use herald_flows::{FollowError, Followed, Verified, followed, prepare};
use herald_ports::Subscriptions;
use herald_testing::{FakeSecrets, FakeSigner};

use crate::activitypub::{activitypub, follower};
use crate::harness::*;

const ACTOR: &str = "https://herald.example/users/php-mutation-gate";
const ANA: &str = "https://social.example/users/ana";
const ANA_INBOX: &str = "https://social.example/users/ana/inbox";
const SHARED: &str = "https://social.example/inbox";

fn ana(inbox: &str) -> String {
    serde_json::json!({
        "id": ANA,
        "type": "Person",
        "inbox": inbox,
        "endpoints": { "sharedInbox": SHARED },
    })
    .to_string()
}

fn follow(object: &str) -> Vec<u8> {
    serde_json::json!({ "id": "https://social.example/f/1", "type": "Follow", "actor": ANA, "object": object })
        .to_string()
        .into_bytes()
}

fn undo(object: &str) -> Vec<u8> {
    serde_json::json!({
        "id": "https://social.example/u/1",
        "type": "Undo",
        "actor": ANA,
        "object": { "type": "Follow", "actor": ANA, "object": object },
    })
    .to_string()
    .into_bytes()
}

fn signed_by(signer: Option<&str>) -> Verified {
    Verified {
        delivery: None,
        signer: signer.map(ToOwned::to_owned),
    }
}

fn taken(
    store: &Failing,
    script: &Script,
    verified: &Verified,
    body: &[u8],
) -> Result<Followed, FollowError> {
    let config = activitypub();
    let secrets = FakeSecrets::default().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    let prepared = pollster::block_on(prepare(&config, &secrets, &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    pollster::block_on(followed(
        &connected,
        store,
        &FakeSigner,
        NOON,
        verified,
        body,
    ))
}

fn served(answers: &[(&str, u16, &str)]) -> Script {
    Script::answering(answers).headed(&[("content-type", "application/activity+json")])
}

fn kept(store: &Failing) -> Vec<herald_core::Subscription> {
    pollster::block_on(
        store
            .inner
            .subscribers(&DestinationName::new("fedi"), "php-mutation-gate"),
    )
    .unwrap()
}

#[test]
fn a_follow_keeps_its_follower_at_its_shared_inbox_and_is_accepted_at_its_own() {
    let store = healthy();
    let document = ana(ANA_INBOX);
    let script = served(&[(ANA, 200, &document), (ANA_INBOX, 202, "")]);
    assert_eq!(
        taken(&store, &script, &signed_by(Some(ANA)), &follow(ACTOR)),
        Ok(Followed::Accepted {
            follower: ANA.to_owned(),
            actor: "php-mutation-gate".to_owned(),
            answered: Sent::Landed(Facts::default()),
        })
    );
    assert_eq!(
        kept(&store),
        vec![follower("php-mutation-gate", ANA, Some(SHARED))]
    );
    assert_eq!(
        script.seen(),
        vec![format!("GET {ANA}"), format!("POST {ANA_INBOX}")]
    );
    let accepted: serde_json::Value = serde_json::from_slice(&script.bodies()[1]).unwrap();
    assert_eq!(accepted["type"], "Accept");
    assert_eq!(accepted["actor"], ACTOR);
    assert_eq!(accepted["object"]["id"], "https://social.example/f/1");
}

#[test]
fn a_follower_is_kept_though_its_inbox_does_not_take_the_accept() {
    let store = healthy();
    let document = ana(ANA_INBOX);
    let script = served(&[(ANA, 200, &document), (ANA_INBOX, 403, "")]);
    let Ok(Followed::Accepted { answered, .. }) =
        taken(&store, &script, &signed_by(Some(ANA)), &follow(ACTOR))
    else {
        panic!("the follow is taken");
    };
    assert_eq!(
        answered,
        Sent::Refused("the inbox refused it with 403".to_owned())
    );
    assert_eq!(kept(&store).len(), 1);
}

#[test]
fn a_follow_of_an_actor_herald_does_not_serve_asks_nothing_of_anyone() {
    let store = healthy();
    let script = served(&[]);
    assert_eq!(
        taken(
            &store,
            &script,
            &signed_by(Some(ANA)),
            &follow("https://herald.example/users/someone-else"),
        ),
        Ok(Followed::Ignored(
            "it follows no actor herald serves".to_owned()
        ))
    );
    assert_eq!(script.seen(), Vec::<String>::new());
    assert_eq!(kept(&store), vec![]);
}

#[test]
fn a_follower_whose_inbox_is_on_another_origin_is_not_kept() {
    let store = healthy();
    let document = ana("https://elsewhere.example/inbox");
    let script = served(&[(ANA, 200, &document)]);
    assert_eq!(
        taken(&store, &script, &signed_by(Some(ANA)), &follow(ACTOR)),
        Ok(Followed::Ignored(
            "the follower's document names no inbox on its own origin".to_owned()
        ))
    );
    assert_eq!(script.seen(), vec![format!("GET {ANA}")]);
    assert_eq!(kept(&store), vec![]);
}

#[test]
fn a_follower_whose_document_cannot_be_read_is_not_kept() {
    let store = healthy();
    let script = served(&[(ANA, 404, "")]);
    assert_eq!(
        taken(&store, &script, &signed_by(Some(ANA)), &follow(ACTOR)),
        Err(FollowError::Unfetched("its server answered 404".to_owned()))
    );
    assert_eq!(kept(&store), vec![]);
}

#[test]
fn a_follow_that_could_not_be_taken_reads_as_a_sentence() {
    assert_eq!(
        FollowError::Store("broken".to_owned()).to_string(),
        "the follower could not be kept: broken"
    );
    assert_eq!(
        FollowError::Unfetched("its server answered 404".to_owned()).to_string(),
        "the follower's document could not be read: its server answered 404"
    );
}

#[test]
fn a_follow_the_store_cannot_keep_is_not_accepted() {
    let mut store = healthy();
    store.fault = Fault::Subscribes;
    let document = ana(ANA_INBOX);
    let script = served(&[(ANA, 200, &document)]);
    let taken = taken(&store, &script, &signed_by(Some(ANA)), &follow(ACTOR));
    assert_eq!(
        taken,
        Err(FollowError::Store(
            "the store could not be reached: broken".to_owned()
        ))
    );
    assert_eq!(
        taken.unwrap_err().to_string(),
        "the follower could not be kept: the store could not be reached: broken"
    );
    assert_eq!(script.seen(), vec![format!("GET {ANA}")]);
}

#[test]
fn an_undo_forgets_only_the_follower_who_sent_it() {
    let store = healthy();
    let bo = follower(
        "php-mutation-gate",
        "https://social.example/users/bo",
        Some(SHARED),
    );
    for subscription in [follower("php-mutation-gate", ANA, Some(SHARED)), bo.clone()] {
        pollster::block_on(store.subscribe(&subscription)).unwrap();
    }
    let script = served(&[]);
    assert_eq!(
        taken(&store, &script, &signed_by(Some(ANA)), &undo(ACTOR)),
        Ok(Followed::Undone {
            follower: ANA.to_owned(),
            actor: "php-mutation-gate".to_owned(),
        })
    );
    assert_eq!(kept(&store), vec![bo]);
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn an_undo_the_store_cannot_forget_leaves_the_follower_kept() {
    let mut store = healthy();
    let ana_follows = follower("php-mutation-gate", ANA, Some(SHARED));
    pollster::block_on(store.subscribe(&ana_follows)).unwrap();
    store.fault = Fault::Unsubscribes;
    let script = served(&[]);
    assert_eq!(
        taken(&store, &script, &signed_by(Some(ANA)), &undo(ACTOR)),
        Err(FollowError::Store(
            "the store could not be reached: broken".to_owned()
        ))
    );
    assert_eq!(kept(&store), vec![ana_follows]);
}

#[test]
fn a_follower_is_kept_though_its_inbox_cannot_be_reached_for_the_accept() {
    let store = healthy();
    let document = ana(ANA_INBOX);
    let script = served(&[(ANA, 200, &document)]);
    let Ok(Followed::Accepted { answered, .. }) =
        taken(&store, &script, &signed_by(Some(ANA)), &follow(ACTOR))
    else {
        panic!("the follow is taken");
    };
    let Sent::Refused(reason) = answered else {
        panic!("{answered:?}");
    };
    assert!(reason.contains("nothing scripted"), "{reason}");
    assert_eq!(kept(&store).len(), 1);
}

#[test]
fn an_undo_the_store_cannot_read_forgets_no_one() {
    let mut store = healthy();
    pollster::block_on(store.subscribe(&follower("php-mutation-gate", ANA, Some(SHARED)))).unwrap();
    store.fault = Fault::Subscribers;
    let script = served(&[]);
    assert_eq!(
        taken(&store, &script, &signed_by(Some(ANA)), &undo(ACTOR)),
        Err(FollowError::Store(
            "the store could not be reached: broken".to_owned()
        ))
    );
    assert_eq!(kept(&store).len(), 1);
}

#[test]
fn an_activity_without_a_signer_or_for_no_served_actor_is_left_alone() {
    let store = healthy();
    let script = served(&[]);
    assert_eq!(
        taken(&store, &script, &signed_by(None), &follow(ACTOR)),
        Ok(Followed::Ignored("it names no signer".to_owned()))
    );
    assert_eq!(
        taken(
            &store,
            &script,
            &signed_by(Some(ANA)),
            &undo("https://herald.example/users/someone-else"),
        ),
        Ok(Followed::Ignored(
            "it undoes a follow of no actor herald serves".to_owned()
        ))
    );
    assert_eq!(
        taken(
            &store,
            &script,
            &signed_by(Some(ANA)),
            br#"{"type":"Like"}"#
        ),
        Ok(Followed::Ignored("herald takes no Like".to_owned()))
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}
