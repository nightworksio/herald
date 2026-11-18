//! Each peer's rates on its pushes: its own, its replays' and the
//! strangers', the nonces of requests refused past them kept, and the hold
//! that follows too many.

use super::*;

#[test]
fn a_peer_past_its_rate_is_answered_too_many() {
    let text = pushing("", "").peers["upstream"].rate;
    assert_eq!(text.count.get(), 120);
    let world = World::with(
        {
            let mut config = pushing("", "");
            config.peers.get_mut("upstream").unwrap().rate =
                herald_core::Rate::new(std::num::NonZeroU32::MIN, herald_core::Per::Minute);
            config
        },
        Script::default(),
    );
    assert_eq!(
        pushed(&world, &release("github.com/1/v1"), &nonce())
            .reply
            .status,
        202
    );
    let limited = pushed(&world, &release("github.com/1/v2"), &nonce());
    assert_eq!(
        said(&limited),
        (
            429,
            TEXT,
            "the peer pushes more often than herald takes".to_owned()
        )
    );
    assert_eq!(
        audit(&world, "upstream")[1],
        (
            429,
            "past its rate; no more of its requests are kept in this window".to_owned()
        )
    );
    assert_eq!(
        pushed(&world, &release("github.com/1/v3"), &nonce())
            .reply
            .status,
        429
    );
    assert_eq!(audit(&world, "upstream").len(), 2);
}

#[test]
fn every_fresh_request_a_peer_signs_counts_against_its_rate_however_the_source_answers_it() {
    let mut config = pushing("", "");
    config.peers.get_mut("upstream").unwrap().rate = herald_core::Rate::new(
        std::num::NonZeroU32::new(2).unwrap(),
        herald_core::Per::Minute,
    );
    let world = World::with(config, Script::default());
    let unread = b"{}";
    assert_eq!(pushed(&world, unread, &nonce()).reply.status, 403);
    let body = release("github.com/1/v1");
    assert_eq!(pushed(&world, &body, &nonce()).reply.status, 202);
    assert_eq!(pushed(&world, &body, &nonce()).reply.status, 429);
    assert_eq!(pushed(&world, &body, &nonce()).reply.status, 429);
    let statuses: Vec<u16> = audit(&world, "upstream")
        .iter()
        .map(|(status, _)| *status)
        .collect();
    assert_eq!(statuses, [403, 202, 429]);
}

#[test]
fn replaying_a_captured_push_spends_only_the_peer_s_replays_never_its_rate() {
    let mut config = pushing("", "");
    let minute = |count: u32| {
        herald_core::Rate::new(
            std::num::NonZeroU32::new(count).unwrap(),
            herald_core::Per::Minute,
        )
    };
    config.peers.get_mut("upstream").unwrap().rate = minute(2);
    config.federation.replay_rate = minute(1);
    let world = World::with(config, Script::default());
    let body = release("github.com/1/v1");
    let headers = signed(&body, (&OLD, &OLD), Some(nonce().as_str()));
    assert_eq!(push(&world, &body, &headers).reply.status, 202);
    let replays: Vec<u16> = (0..4)
        .map(|_| push(&world, &body, &headers).reply.status)
        .collect();
    assert_eq!(replays, [401, 429, 429, 429]);
    assert_eq!(
        pushed(&world, &release("github.com/1/v2"), &nonce())
            .reply
            .status,
        202
    );
    let kept = audit(&world, "upstream");
    assert_eq!(
        kept.iter().map(|(status, _)| *status).collect::<Vec<_>>(),
        [202, 401, 429, 202]
    );
    assert_eq!(
        kept[2].1,
        "past the rate of replayed nonces; no more replays are kept in this window"
    );
}

#[test]
fn a_push_racing_another_with_its_nonce_is_taken_once() {
    let mut world = world();
    world.store.fault = Fault::SeesNoDeliveries;
    let body = release("github.com/1/v1");
    let headers = signed(&body, (&OLD, &OLD), Some(nonce().as_str()));
    let first = push(&world, &body, &headers);
    assert_eq!(first.reply.status, 202);
    assert!(first.job.is_some());
    let raced = push(&world, &body, &headers);
    assert_eq!(raced.reply.status, 401);
    assert_eq!(raced.refused.as_deref(), Some("the nonce was seen before"));
    assert!(raced.job.is_none());
    assert_eq!(
        audit(&world, "upstream"),
        [
            (202, "taken".to_owned()),
            (401, "the nonce was seen before".to_owned())
        ]
    );
}

#[test]
fn a_signature_naming_a_peer_that_does_not_hold_spends_none_of_its_rate() {
    let mut config = pushing("", "");
    let one = herald_core::Rate::new(std::num::NonZeroU32::MIN, herald_core::Per::Minute);
    config.peers.get_mut("upstream").unwrap().rate = one;
    config.federation.stranger_rate = one;
    let world = World::with(config, Script::default());
    let body = release("github.com/1/v1");
    for _ in 0..3 {
        let forged = push(
            &world,
            &body,
            &signed(&body, (&OLD, &STRANGER), Some(nonce().as_str())),
        );
        assert_eq!(forged.reply.status, 401);
        assert_eq!(
            forged.refused.as_deref(),
            Some("the signature does not hold")
        );
    }
    let strangers = pollster::block_on(world.store.inner.audits(AuditDay::STRANGERS)).unwrap();
    let kept: Vec<&str> = strangers
        .iter()
        .flat_map(|day| &day.entries)
        .map(|entry| entry.reason.as_str())
        .collect();
    assert_eq!(
        kept,
        [
            "the signature does not hold",
            "the signature does not hold; past the strangers' rate, no more are kept in this window"
        ]
    );
    assert_eq!(pushed(&world, &body, &nonce()).reply.status, 202);
    assert_eq!(audit(&world, "upstream"), [(202, "taken".to_owned())]);
    let mut unread = World::with(pushing("", ""), Script::default());
    unread.store.fault = Fault::ReadsWindows;
    let forged = push(
        &unread,
        &body,
        &signed(&body, (&OLD, &STRANGER), Some(nonce().as_str())),
    );
    assert_eq!(
        forged.refused.as_deref(),
        Some(
            "the signature does not hold; the audit was not kept: the store could not be reached: broken"
        )
    );
}

/// A world whose peer may push once a minute, and have the nonces of
/// `refused` requests refused past that kept.
fn once_a_minute(refused: u32) -> World {
    let mut config = pushing("", "");
    config.peers.get_mut("upstream").unwrap().rate =
        herald_core::Rate::new(std::num::NonZeroU32::MIN, herald_core::Per::Minute);
    config.federation.refused_nonces = refused;
    World::with(config, Script::default())
}

fn window(world: &World) -> herald_ports::Versioned<herald_core::Window> {
    pollster::block_on(world.store.inner.window("upstream"))
        .unwrap()
        .unwrap()
}

/// How long past its expiry a signature is still taken.
fn skew() -> jiff::SignedDuration {
    jiff::SignedDuration::from_secs(pushing("", "").server.inbound.replay_window.as_secs())
}

#[test]
fn a_request_refused_past_the_rate_is_never_taken_later() {
    let world = once_a_minute(10);
    assert_eq!(
        pushed(&world, &release("github.com/1/v1"), &nonce())
            .reply
            .status,
        202
    );
    let body = release("github.com/1/v2");
    let once = nonce();
    let headers = signed(&body, (&OLD, &OLD), Some(&once));
    assert_eq!(push(&world, &body, &headers).reply.status, 429);
    assert!(kept_nonce(&world, "upstream", &once));
    let past = pushed(&world, &body, &nonce());
    assert_eq!(past.refused.as_deref(), Some("upstream: past its rate"));
    world.at(at() + jiff::SignedDuration::from_mins(2));
    let replayed = push(&world, &body, &headers);
    assert_eq!(replayed.reply.status, 401);
    assert_eq!(
        replayed.refused.as_deref(),
        Some("the nonce was seen before")
    );
    let taken = audit(&world, "upstream")
        .iter()
        .filter(|(status, _)| *status == 202)
        .count();
    assert_eq!(taken, 1);
}

#[test]
fn a_peer_past_its_refused_nonces_is_held_until_its_requests_expire() {
    let world = once_a_minute(1);
    let statuses: Vec<u16> = (0..2)
        .map(|_| {
            pushed(&world, &release("github.com/1/v1"), &nonce())
                .reply
                .status
        })
        .collect();
    assert_eq!(statuses, [202, 429]);
    let body = release("github.com/1/v2");
    let unkept = nonce();
    let headers = signed(&body, (&OLD, &OLD), Some(&unkept));
    assert_eq!(push(&world, &body, &headers).reply.status, 429);
    assert!(!kept_nonce(&world, "upstream", &unkept));
    let hold = window(&world);
    let until =
        at() + jiff::SignedDuration::from_mins(5) + skew() + jiff::SignedDuration::from_secs(1);
    assert_eq!(hold.value.held, Some(until));
    let during: Vec<String> = (0..3).map(|_| nonce()).collect();
    for once in &during {
        let refused = pushed(&world, &release("github.com/1/v3"), once);
        assert_eq!(refused.reply.status, 429);
        assert_eq!(
            refused.refused.as_deref(),
            Some("upstream: held until every request it made has expired")
        );
    }
    assert!(
        during
            .iter()
            .all(|once| !kept_nonce(&world, "upstream", once))
    );
    assert_eq!(window(&world), hold);
    let kept = audit(&world, "upstream");
    assert_eq!(kept.len(), 3);
    assert_eq!(
        kept[2],
        (
            429,
            "past its rate and the refused requests kept; held, no more of its requests are kept until every one it made has expired".to_owned()
        )
    );
    world.at(until - jiff::SignedDuration::from_secs(1));
    let early = pushed(&world, &release("github.com/1/v4"), &nonce());
    assert_eq!(early.reply.status, 429);
    assert_eq!(window(&world), hold);
    world.at(until);
    let expired = push(&world, &body, &headers);
    assert_eq!(expired.reply.status, 401);
    assert_eq!(
        expired.refused.as_deref(),
        Some(herald_peers::message::Refusal::Expired.to_string().as_str())
    );
    assert!(!kept_nonce(&world, "upstream", &unkept));
    let fresh = release("github.com/1/v5");
    let taken = push(
        &world,
        &fresh,
        &signed_then(until, URL, &fresh, (&OLD, &OLD), Some(nonce().as_str())),
    );
    assert_eq!(taken.reply.status, 202);
}

#[test]
fn a_held_peer_s_later_request_lengthens_its_hold_and_keeps_nothing_else() {
    let world = once_a_minute(1);
    for status in [202, 429, 429] {
        let answered = pushed(&world, &release("github.com/1/v1"), &nonce());
        assert_eq!(answered.reply.status, status);
    }
    let later = at() + jiff::SignedDuration::from_mins(1);
    world.at(later);
    let body = release("github.com/1/v2");
    let once = nonce();
    let held = push(
        &world,
        &body,
        &signed_then(later, URL, &body, (&OLD, &OLD), Some(&once)),
    );
    assert_eq!(held.reply.status, 429);
    assert_eq!(
        window(&world).value.held,
        Some(
            later
                + jiff::SignedDuration::from_mins(5)
                + skew()
                + jiff::SignedDuration::from_secs(1)
        )
    );
    assert!(!kept_nonce(&world, "upstream", &once));
    assert_eq!(audit(&world, "upstream").len(), 3);
}

#[test]
fn refused_nonces_are_counted_over_the_longest_a_request_lives() {
    let world = once_a_minute(1);
    for status in [202, 429] {
        let answered = pushed(&world, &release("github.com/1/v1"), &nonce());
        assert_eq!(answered.reply.status, status);
    }
    let later = at() + jiff::SignedDuration::from_mins(2);
    world.at(later);
    let push_then = |identity: &str| {
        let body = release(identity);
        push(
            &world,
            &body,
            &signed_then(later, URL, &body, (&OLD, &OLD), Some(nonce().as_str())),
        )
    };
    assert_eq!(push_then("github.com/1/v2").reply.status, 202);
    let held = push_then("github.com/1/v3");
    assert_eq!(held.reply.status, 429);
    assert!(window(&world).value.held.is_some());
}
