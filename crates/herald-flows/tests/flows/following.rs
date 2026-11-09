use herald_core::{DestinationName, Facts, Sent};
use herald_flows::{
    Checking, Decision, FollowError, Followed, Verified, decide, followed, followers, prepare,
};
use herald_ports::Subscriptions;
use herald_testing::{FakeSecrets, FakeSigner};

use crate::activitypub::{activitypub, follower, governed};
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
    taken_under(&activitypub(), store, script, verified, body)
}

fn taken_under(
    config: &herald_config::Config,
    store: &Failing,
    script: &Script,
    verified: &Verified,
    body: &[u8],
) -> Result<Followed, FollowError> {
    let secrets = FakeSecrets::default()
        .with("HERALD_ACTIVITYPUB_KEY", "the-key")
        .with("HERALD_DISCORD_TOKEN", "hunter2");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    pollster::block_on(followed(
        &connected,
        store,
        &FakeSigner,
        Checking {
            now: NOON,
            bounds: &herald_core::Inbound::DEFAULT,
        },
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
fn a_follower_the_actor_s_lists_turn_away_is_ignored_and_asked_nothing() {
    for policy in [
        r#"block = ["social.example"]"#,
        r#"block = ["https://social.example/users/ana"]"#,
        r#"mode = "allowlist"
allow = ["friends.example"]"#,
    ] {
        let store = healthy();
        let script = served(&[]);
        assert_eq!(
            taken_under(
                &governed(policy),
                &store,
                &script,
                &signed_by(Some(ANA)),
                &follow(ACTOR)
            ),
            Ok(Followed::Ignored(
                "the actor's block or allow list turns its follower away".to_owned()
            )),
            "{policy}"
        );
        assert!(script.seen().is_empty(), "{policy}");
        assert!(kept(&store).is_empty(), "{policy}");
    }
}

#[test]
fn a_follow_past_the_actor_s_caps_is_rejected_and_its_follower_not_kept() {
    let store = healthy();
    let bo = follower(
        "php-mutation-gate",
        "https://social.example/users/bo",
        Some(SHARED),
    );
    pollster::block_on(store.subscribe(&bo)).unwrap();
    let document = ana(ANA_INBOX);
    for caps in ["followers = { most = 1 }", "followers = { per_domain = 1 }"] {
        let script = served(&[(ANA, 200, &document), (ANA_INBOX, 202, "")]);
        assert_eq!(
            taken_under(
                &governed(caps),
                &store,
                &script,
                &signed_by(Some(ANA)),
                &follow(ACTOR)
            ),
            Ok(Followed::Rejected {
                follower: ANA.to_owned(),
                actor: "php-mutation-gate".to_owned(),
                answered: Sent::Landed(Facts::default()),
            }),
            "{caps}"
        );
        let rejected: serde_json::Value = serde_json::from_slice(&script.bodies()[1]).unwrap();
        assert_eq!(rejected["type"], "Reject");
        assert_eq!(rejected["actor"], ACTOR);
        assert_eq!(rejected["object"]["id"], "https://social.example/f/1");
        assert!(
            rejected["id"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{ACTOR}/rejects/"))
        );
    }
    assert_eq!(kept(&store), vec![bo.clone()]);
    let open = served(&[(ANA, 200, &document), (ANA_INBOX, 202, "")]);
    let room = governed("followers = { most = 2, per_domain = 2 }");
    assert!(matches!(
        taken_under(&room, &store, &open, &signed_by(Some(ANA)), &follow(ACTOR)),
        Ok(Followed::Accepted { .. })
    ));
    let again = served(&[(ANA, 200, &document), (ANA_INBOX, 202, "")]);
    let full = governed("followers = { most = 1 }");
    assert!(
        matches!(
            taken_under(&full, &store, &again, &signed_by(Some(ANA)), &follow(ACTOR)),
            Ok(Followed::Accepted { .. })
        ),
        "a follower already kept is accepted again past the caps"
    );
}

#[test]
fn a_follower_the_actor_s_lists_turn_away_takes_no_place_under_its_caps() {
    let store = healthy();
    let spam = follower(
        "php-mutation-gate",
        "https://spam.example/users/x",
        Some("https://spam.example/inbox"),
    );
    pollster::block_on(store.subscribe(&spam)).unwrap();
    let document = ana(ANA_INBOX);
    let script = served(&[(ANA, 200, &document), (ANA_INBOX, 202, "")]);
    let config = governed(
        r#"block = ["spam.example"]
followers = { most = 1 }"#,
    );
    assert!(matches!(
        taken_under(
            &config,
            &store,
            &script,
            &signed_by(Some(ANA)),
            &follow(ACTOR)
        ),
        Ok(Followed::Accepted { .. })
    ));
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
    assert_eq!(
        taken_under(
            &config(""),
            &store,
            &script,
            &signed_by(Some(ANA)),
            &follow(ACTOR)
        ),
        Ok(Followed::Ignored("herald serves no actor".to_owned()))
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}

fn decided(
    config: &herald_config::Config,
    store: &Failing,
    script: &Script,
    actor: &str,
    decision: Decision,
) -> Result<Sent, FollowError> {
    let secrets = FakeSecrets::default()
        .with("HERALD_ACTIVITYPUB_KEY", "the-key")
        .with("HERALD_DISCORD_TOKEN", "hunter2");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    pollster::block_on(decide(
        &connected,
        store,
        &FakeSigner,
        NOON,
        &DestinationName::new("fedi"),
        actor,
        ANA,
        decision,
    ))
}

fn waiting(store: &Failing) -> herald_config::Config {
    let manual = governed(r#"approve = "manual""#);
    let document = ana(ANA_INBOX);
    for _ in 0..2 {
        let script = served(&[(ANA, 200, &document)]);
        assert_eq!(
            taken_under(
                &manual,
                store,
                &script,
                &signed_by(Some(ANA)),
                &follow(ACTOR)
            ),
            Ok(Followed::Waiting {
                follower: ANA.to_owned(),
                actor: "php-mutation-gate".to_owned(),
            })
        );
        assert_eq!(script.seen(), vec![format!("GET {ANA}")]);
    }
    manual
}

#[test]
fn a_follow_an_actor_approves_by_hand_waits_unanswered_and_unaccepted() {
    let store = healthy();
    waiting(&store);
    assert_eq!(
        kept(&store),
        vec![herald_core::Subscription {
            pending: Some("https://social.example/f/1".to_owned()),
            ..follower("php-mutation-gate", ANA, Some(SHARED))
        }]
    );
}

#[test]
fn an_approved_follow_is_kept_and_accepted_at_its_inbox() {
    let store = healthy();
    let manual = waiting(&store);
    let script = served(&[(SHARED, 202, "")]);
    assert_eq!(
        decided(
            &manual,
            &store,
            &script,
            "php-mutation-gate",
            Decision::Approve
        ),
        Ok(Sent::Landed(Facts::default()))
    );
    assert_eq!(
        kept(&store),
        vec![follower("php-mutation-gate", ANA, Some(SHARED))]
    );
    let accepted: serde_json::Value = serde_json::from_slice(&script.bodies()[0]).unwrap();
    assert_eq!(accepted["type"], "Accept");
    assert_eq!(
        accepted["object"],
        serde_json::json!({
            "id": "https://social.example/f/1",
            "type": "Follow",
            "actor": ANA,
            "object": ACTOR,
        })
    );
    let document = ana(ANA_INBOX);
    let again = served(&[(ANA, 200, &document), (ANA_INBOX, 202, "")]);
    assert!(matches!(
        taken_under(
            &manual,
            &store,
            &again,
            &signed_by(Some(ANA)),
            &follow(ACTOR)
        ),
        Ok(Followed::Accepted { .. })
    ));
    assert_eq!(
        decided(
            &manual,
            &store,
            &served(&[]),
            "php-mutation-gate",
            Decision::Approve
        ),
        Err(FollowError::Unknown(format!(
            "no follow of \"php-mutation-gate\" by \"{ANA}\" waits for approval"
        )))
    );
}

#[test]
fn a_rejected_follow_is_forgotten_and_rejected_at_its_inbox() {
    let store = healthy();
    let manual = waiting(&store);
    let script = served(&[(SHARED, 202, "")]);
    assert_eq!(
        decided(
            &manual,
            &store,
            &script,
            "php-mutation-gate",
            Decision::Reject
        ),
        Ok(Sent::Landed(Facts::default()))
    );
    assert_eq!(kept(&store), []);
    let rejected: serde_json::Value = serde_json::from_slice(&script.bodies()[0]).unwrap();
    assert_eq!(rejected["type"], "Reject");
    assert_eq!(rejected["object"]["id"], "https://social.example/f/1");
}

#[test]
fn a_follow_approved_past_the_caps_or_of_no_actor_is_refused() {
    let store = healthy();
    waiting(&store);
    let bo = follower(
        "php-mutation-gate",
        "https://social.example/users/bo",
        Some(SHARED),
    );
    pollster::block_on(store.subscribe(&bo)).unwrap();
    let full = governed("approve = \"manual\"\nfollowers = { most = 1 }");
    assert_eq!(
        decided(
            &full,
            &store,
            &served(&[]),
            "php-mutation-gate",
            Decision::Approve
        ),
        Err(FollowError::Full)
    );
    assert_eq!(
        FollowError::Full.to_string(),
        "the actor's followers are full; raise followers.most or followers.per_domain, or reject the follow"
    );
    let unknown = decided(&full, &store, &served(&[]), "nobody", Decision::Reject);
    assert_eq!(
        unknown,
        Err(FollowError::Unknown(
            "the destination \"fedi\" serves no actor \"nobody\"".to_owned()
        ))
    );
    assert_eq!(
        unknown.unwrap_err().to_string(),
        "the destination \"fedi\" serves no actor \"nobody\""
    );
    let secrets = FakeSecrets::default()
        .with("HERALD_ACTIVITYPUB_KEY", "the-key")
        .with("HERALD_DISCORD_TOKEN", "hunter2");
    let prepared = pollster::block_on(prepare(&full, &secrets, &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(served(&[]))).unwrap();
    let listed = pollster::block_on(followers(
        &connected,
        &store,
        &DestinationName::new("fedi"),
        "php-mutation-gate",
    ))
    .unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(
        listed.iter().filter(|kept| kept.pending.is_some()).count(),
        1
    );
    assert!(
        pollster::block_on(followers(
            &connected,
            &store,
            &DestinationName::new("fedi"),
            "nobody"
        ))
        .is_err()
    );
}
