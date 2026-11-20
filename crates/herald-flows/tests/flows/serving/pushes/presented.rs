//! A push presenting a CI run's ID token: taken as from the name its app
//! stands for, within that name's rate and the source's grant of it, its
//! envelope proved by this herald itself as of when it is taken, its
//! token taken once, and kept in the name's audit with the token's issuer,
//! id and the claims its entry binds and what it pushed, never the token.

use herald_core::Audited;

use super::*;
use crate::serving::oidc::{
    AUDIENCE, DISCOVERED, ISSUER, KEYS, discovery, header, id_token, key_set, run,
};

const RUN: &str = "plugin-ci";
const RUN_ORIGIN: &str = "https://ci.invalid/.well-known/herald";

fn standing(until: &str) -> Config {
    loaded(&standing_text(until))
}

fn loaded(text: &str) -> Config {
    load(
        "herald.toml",
        text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

/// The config of an app standing for [`RUN`], which `inbox` grants a push
/// until `until`.
fn standing_text(until: &str) -> String {
    format!(
        r#"version = 1
[store]
kind = "sqlite"
[federation]
domain = "herald.example"
keys = [{{ secret = "HERALD_KEY_NEW" }}]
[apps.runs]
oidc = [{{ issuer = "{ISSUER}", audience = "{AUDIENCE}", claims = {{ repository_id = "42" }} }}]
peers = ["{RUN}", "docs-ci"]
[destinations.releases]
kind = "herald"
[sources.inbox]
kind = "herald"
peers = {{ {RUN} = {{ actions = ["push"], namespaces = ["github.com/42"]{until} }} }}
[[routes]]
id = "mirror"
sources = ["inbox"]
destination = "releases"
private = false
"#
    )
}

/// A world whose issuer answers its discovery document and keys once.
fn fetching(until: &str) -> World {
    let (discovered, keys) = (discovery(), key_set(&["k1"]));
    World::with(
        standing(until),
        Script::answering(&[(DISCOVERED, 200, &discovered), (KEYS, 200, &keys)]),
    )
    .sized()
}

fn presented(world: &World, path: &str, token: &str, body: &[u8]) -> Answered {
    let authorization = format!("Bearer {token}");
    world.answer(&Inbound {
        method: "POST",
        path,
        query: None,
        headers: &[
            ("Authorization", &authorization),
            ("host", "herald.example"),
            ("content-type", "application/json"),
        ],
        body,
    })
}

fn release_of(identity: &str) -> Vec<u8> {
    envelope(identity, &[RUN_ORIGIN]).to_string().into_bytes()
}

fn entries(world: &World, peer: &str) -> Vec<Audited> {
    pollster::block_on(world.store.inner.audits(peer))
        .unwrap()
        .into_iter()
        .flat_map(|day| day.entries)
        .collect()
}

/// A fresh token of a run of the repository 42.
fn token() -> String {
    id_token(&header("k1"), &run("42"))
}

#[test]
fn a_run_s_push_is_taken_as_its_name_proved_by_this_herald_and_audited_with_its_claims() {
    let world = fetching("");
    let mut claims = run("42");
    claims["jti"] = json!("first-run");
    claims["run_id"] = json!(7);
    claims["repository_visibility"] = json!("private");
    let token = id_token(&header("k1"), &claims);
    let body = release_of("github.com/42/v1");
    let first = presented(&world, INBOX, &token, &body);
    assert_eq!(
        said(&first),
        (202, TEXT, "herald took the envelope".to_owned()),
        "{:?}",
        first.refused
    );
    let Some(Job::Pushed {
        event: Some(event), ..
    }) = first.job
    else {
        panic!("the run's push is left to announce");
    };
    assert_eq!(event.identity().as_str(), "github.com/42/v1");
    let kept = cursor(&world).unwrap();
    let proof = kept
        .proof(&herald_core::Identity::new("github.com/42/v1"))
        .unwrap();
    assert_eq!(proof.created, at());
    assert_eq!(proof.maker, RUN);
    let foreign = presented(
        &world,
        INBOX,
        &self::token(),
        &release_of("github.com/9/v1"),
    );
    let Some(Job::Pushed {
        event: Some(event), ..
    }) = foreign.job
    else {
        panic!("{:?}", foreign.job);
    };
    assert_eq!(
        event.identity().as_str(),
        "herald/plugin-ci/github.com/9/v1"
    );
    let again = presented(&world, INBOX, &self::token(), &body);
    assert_eq!(said(&again).0, 202);
    assert_eq!(again.job, None);
    let audited = entries(&world, RUN);
    assert_eq!(audited.len(), 3);
    let entry = &audited[0];
    assert_eq!(entry.peer.as_deref(), Some(RUN));
    assert_eq!(entry.keyid.as_deref(), Some("oidc https://issuer.example"));
    assert_eq!(
        entry.evidence,
        std::collections::BTreeMap::from([
            ("iss".to_owned(), ISSUER.to_owned()),
            ("jti".to_owned(), "first-run".to_owned()),
            ("repository_id".to_owned(), "42".to_owned()),
            ("run_id".to_owned(), "7".to_owned()),
        ])
    );
    assert_eq!(entry.identity.as_deref(), Some("github.com/42/v1"));
    assert_eq!(
        audited[1].identity.as_deref(),
        Some("herald/plugin-ci/github.com/9/v1")
    );
    assert_eq!(audited[2].identity.as_deref(), Some("github.com/42/v1"));
    assert_eq!(
        entry.digest,
        Some(herald_peers::digest::content_digest(&body))
    );
    let written = serde_json::to_string(&audited).unwrap();
    assert!(!written.contains(&token), "{written}");
    assert!(!written.contains("org/plugin"), "{written}");
    assert!(!written.contains("private"), "{written}");
    assert_eq!(world.script.seen(), [DISCOVERED, KEYS]);
}

#[test]
fn a_run_s_token_pushes_once_even_where_what_it_brought_was_refused() {
    let world = fetching("");
    let once = token();
    let unread = presented(&world, INBOX, &once, b"[1]");
    assert_eq!(said(&unread).0, 403);
    let retried = presented(&world, INBOX, &once, &release_of("github.com/42/v1"));
    assert_eq!(said(&retried).0, 401);
    assert_eq!(
        retried.refused.as_deref(),
        Some("the ID token was taken before, and each is taken once")
    );
    assert_eq!(retried.job, None);
    assert_eq!(cursor(&world), None);
    let unaudited = entries(&world, RUN);
    assert_eq!(unaudited.len(), 1);
    assert_eq!(
        (
            unaudited[0].identity.as_ref(),
            unaudited[0].digest.is_some()
        ),
        (None, true)
    );
}

#[test]
fn a_run_withdrawing_what_it_pushed_a_moment_ago_is_taken() {
    let world = fetching("");
    let pushed = presented(&world, INBOX, &token(), &release_of("github.com/42/v1"));
    assert_eq!(said(&pushed).0, 202);
    world.at(at() + jiff::SignedDuration::from_secs(1));
    let tombstone = json!({
        "type": herald_peers::feed::TOMBSTONE,
        "identity": "github.com/42/v1",
        "deleted": "2026-11-17T10:00:01Z",
        "path": [RUN_ORIGIN],
    })
    .to_string();
    let withdrawn = presented(&world, INBOX, &token(), tombstone.as_bytes());
    assert_eq!(said(&withdrawn).0, 202, "{:?}", withdrawn.refused);
    let Some(Job::Pushed {
        withdrawn: Some((identity, _)),
        ..
    }) = withdrawn.job
    else {
        panic!("the withdrawal is left to announce: {:?}", withdrawn.job);
    };
    assert_eq!(identity.as_str(), "github.com/42/v1");
}

#[test]
fn a_run_s_push_that_does_not_hold_is_refused_and_kept_only_once_counted() {
    let world = fetching("");
    let large = vec![b' '; herald_config::Peer::MOST + 1];
    assert_eq!(said(&presented(&world, INBOX, &token(), &large)).0, 413);
    let untaken = id_token(&header("k1"), &run("43"));
    let refused = presented(&world, INBOX, &untaken, &release_of("github.com/42/v1"));
    assert_eq!(said(&refused).0, 401);
    assert!(
        refused
            .refused
            .as_deref()
            .is_some_and(|why| why.contains("repository_id") && !why.contains(&untaken))
    );
    let elsewhere = presented(
        &world,
        "/herald/v1/inbox/other",
        &token(),
        &release_of("github.com/42/v1"),
    );
    assert_eq!(said(&elsewhere).0, 403);
    assert_eq!(
        elsewhere.refused.as_deref(),
        Some("runs: the source grants no name the app stands for")
    );
    assert!(kept_nothing(&world, RUN) && kept_nothing(&world, "runs"));
    let unread = presented(&world, INBOX, &token(), b"[1]");
    assert_eq!(said(&unread).0, 403);
    assert_eq!(
        entries(&world, RUN)
            .iter()
            .map(|entry| (
                entry.outcome,
                entry.reason.starts_with("the body is no envelope: ")
            ))
            .collect::<Vec<_>>(),
        [(403, true)]
    );
    let mut whole = release_of("github.com/42/v0");
    whole.resize(herald_config::Peer::MOST, b' ');
    assert_ne!(said(&presented(&world, INBOX, &token(), &whole)).0, 413);
}

#[test]
fn a_run_s_grant_that_ended_takes_nothing_of_it() {
    let world = fetching(", until = \"2020-01-01T00:00:00Z\"");
    let ended = presented(&world, INBOX, &token(), &release_of("github.com/42/v1"));
    assert_eq!(said(&ended).0, 403);
    assert_eq!(
        audit(&world, RUN),
        [(
            403,
            "the peer's grant to push to the source has ended".to_owned()
        )]
    );
    assert_eq!(cursor(&world), None);
}

#[test]
fn a_run_past_its_name_s_rate_is_refused_and_audited_once() {
    let world = fetching("");
    let body = release_of("github.com/42/v1");
    for _ in 0..120 {
        assert_eq!(said(&presented(&world, INBOX, &token(), &body)).0, 202);
    }
    for _ in 0..2 {
        assert_eq!(said(&presented(&world, INBOX, &token(), &body)).0, 429);
    }
    let audited = audit(&world, RUN);
    assert_eq!(audited.len(), 121);
    assert_eq!(audited[120].0, 429);
}

#[test]
fn a_run_s_push_whose_issuer_s_keys_cannot_be_read_is_refused_for_now() {
    let token = token();
    let unread = World::with(standing(""), Script::default()).sized();
    let answered = presented(&unread, INBOX, &token, &release_of("github.com/42/v1"));
    assert_eq!(said(&answered).0, 503);
    assert!(kept_nothing(&unread, RUN));
    let mut uncounted = fetching("");
    uncounted.store.fault = crate::harness::Fault::WritesWindows;
    let answered = presented(&uncounted, INBOX, &token, &release_of("github.com/42/v1"));
    assert_eq!(said(&answered).0, 503);
    assert!(
        answered
            .refused
            .as_deref()
            .is_some_and(|why| why.starts_with("plugin-ci: ")),
        "{:?}",
        answered.refused
    );
    assert_eq!(cursor(&uncounted), None);
}

#[test]
fn a_run_s_token_does_only_what_its_entry_takes() {
    let both = |takes: &str| {
        let text = standing_text("")
            .replace(
                "peers = [\"plugin-ci\", \"docs-ci\"]",
                "peers = [\"plugin-ci\", \"docs-ci\"]\nprojects = [\"herald\"]",
            )
            .replace(
                "claims = { repository_id = \"42\" } }]",
                &format!("claims = {{ repository_id = \"42\" }}, takes = {takes} }}]"),
            )
            + "[[routes]]\nid = \"api\"\nsources = [\"runs\"]\ndestination = \"releases\"\nprivate = false\n";
        let (discovered, keys) = (discovery(), key_set(&["k1"]));
        World::with(
            loaded(&text),
            Script::answering(&[(DISCOVERED, 200, &discovered), (KEYS, 200, &keys)]),
        )
        .sized()
    };
    let announced = |world: &World| {
        let authorization = format!("Bearer {}", token());
        world.answer(&Inbound {
            method: "POST",
            path: "/api/projects/herald/events",
            query: None,
            headers: &[("Authorization", &authorization)],
            body: br#"{"id": "7", "project": "herald", "title": "Deployed"}"#,
        })
    };
    let pushing = both("[\"push\"]");
    let refused = announced(&pushing);
    assert_eq!(said(&refused).0, 401);
    assert_eq!(
        refused.refused.as_deref(),
        Some("no app takes ID tokens of the token's issuer")
    );
    let pushed = presented(&pushing, INBOX, &token(), &release_of("github.com/42/v1"));
    assert_eq!(said(&pushed).0, 202, "{:?}", pushed.refused);
    let announcing = both("[\"announce\"]");
    assert_eq!(said(&announced(&announcing)).0, 202);
    let unpushed = presented(
        &announcing,
        INBOX,
        &token(),
        &release_of("github.com/42/v1"),
    );
    assert_eq!(said(&unpushed).0, 401);
    let doing_both = both("[\"announce\", \"push\"]");
    assert_eq!(said(&announced(&doing_both)).0, 202);
    assert_eq!(
        said(&presented(
            &doing_both,
            INBOX,
            &token(),
            &release_of("github.com/42/v2")
        ))
        .0,
        202
    );
}

#[test]
fn a_run_s_token_meant_for_another_herald_is_refused() {
    let world = fetching("");
    let mut elsewhere = run("42");
    elsewhere["aud"] = json!("https://other.example");
    let token = id_token(&header("k1"), &elsewhere);
    let answered = presented(&world, INBOX, &token, &release_of("github.com/42/v1"));
    assert_eq!(said(&answered).0, 401);
    assert_eq!(
        answered.refused.as_deref(),
        Some("the ID token is not meant for this herald")
    );
    assert_eq!(world.script.seen(), Vec::<String>::new());
}

#[test]
fn a_token_taken_until_it_expires_proves_as_of_its_issue_and_overwrites_nothing() {
    let text =
        standing_text("").replace("[apps.runs]\n", "[apps.runs]\nreplay = \"until-expiry\"\n");
    let (discovered, keys) = (discovery(), key_set(&["k1"]));
    let world = World::with(
        loaded(&text),
        Script::answering(&[(DISCOVERED, 200, &discovered), (KEYS, 200, &keys)]),
    )
    .sized();
    let captured = token();
    let pushed = presented(&world, INBOX, &captured, &release_of("github.com/42/v1"));
    assert_eq!(said(&pushed).0, 202, "{:?}", pushed.refused);
    let proof = cursor(&world)
        .unwrap()
        .proof(&herald_core::Identity::new("github.com/42/v1"))
        .unwrap()
        .clone();
    assert_eq!(proof.created, at() - jiff::SignedDuration::from_secs(10));
    world.at(at() + jiff::SignedDuration::from_secs(60));
    let mut altered = envelope("github.com/42/v1", &[RUN_ORIGIN]);
    altered["title"] = json!("altered");
    let replayed = presented(&world, INBOX, &captured, altered.to_string().as_bytes());
    assert_eq!(said(&replayed).0, 202, "{:?}", replayed.refused);
    assert_eq!(replayed.job, None);
    let kept = cursor(&world).unwrap();
    assert_eq!(
        kept.proof(&herald_core::Identity::new("github.com/42/v1")),
        Some(&proof)
    );
}
