//! Where a sync stands, kept in the store and taken up again, skipping commands given too long ago.

use super::*;

/// A sync answer starting the next at `next`, each command in `given` as
/// its body and when the homeserver received it, in milliseconds, where
/// it says.
fn synced_at(next: &str, given: &[(&str, Option<i64>)]) -> String {
    let events: Vec<Value> = given
        .iter()
        .enumerate()
        .map(|(index, (body, received))| {
            let mut event = json!({
                "type": "m.room.message",
                "room_id": "!room:matrix.example",
                "event_id": format!("$command{index}"),
                "sender": "@ann:matrix.example",
                "content": { "msgtype": "m.text", "body": body },
            });
            if let Some(received) = received {
                event["origin_server_ts"] = json!(received);
            }
            event
        })
        .collect();
    json!({ "next_batch": next, "rooms": { "join": { "!room:matrix.example": { "timeline": { "events": events }}}}})
        .to_string()
}

/// The milliseconds since the epoch of the harness's clock, `minutes`
/// earlier.
fn minutes_ago(minutes: i64) -> i64 {
    NOW.parse::<jiff::Timestamp>()
        .unwrap()
        .checked_sub(jiff::SignedDuration::from_mins(minutes))
        .unwrap()
        .as_millisecond()
}

fn matrix() -> DestinationName {
    DestinationName::new("matrix")
}

async fn kept_at(world: &World) -> Option<String> {
    world.store.inner.position(&matrix()).await.unwrap()
}

/// What a sync failing with 500 is said as.
const UNSYNCED: &str =
    "matrix: the homeserver could not be synced: Matrix refused it with 500: M_UNKNOWN";

fn unsynced(path: &str) -> Scripted {
    answer(path, 500, r#"{"errcode": "M_UNKNOWN"}"#)
}

#[tokio::test(start_paused = true)]
async fn a_sync_resumes_where_it_was_kept_skipping_what_was_given_too_long_ago() {
    let answers = Answers::of(vec![
        answer(
            "since=s5",
            200,
            &synced_at(
                "s6",
                &[
                    ("!herald changelog", Some(minutes_ago(11))),
                    ("!herald latest", Some(minutes_ago(9))),
                ],
            ),
        ),
        answer("/send/m.room.message/", 200, "{}"),
        answer("since=s6", 200, &synced_at("s7", &[])),
        unsynced("since=s7"),
    ]);
    let world = World::new("matrix", answers, ScriptedSockets::default()).await;
    world
        .store
        .inner
        .put_position(&matrix(), "s5")
        .await
        .unwrap();
    let answered = Cell::new(false);
    let failed = Cell::new(false);
    let heard = world
        .served(4, |line| {
            answered.set(answered.get() || line.starts_with("answered"));
            failed.set(failed.get() || line == UNSYNCED);
            answered.get() && failed.get()
        })
        .await;
    assert_eq!(
        heard
            .iter()
            .filter(|line| !line.starts_with("answered"))
            .collect::<Vec<_>>(),
        [
            "matrix: changelog given at 2027-01-15T07:49:00Z was skipped: it was given before 2027-01-15T07:50:00Z, while no session was held",
            UNSYNCED,
        ]
    );
    assert!(
        heard.contains(&"answered latest: Ok(())".to_owned()),
        "{heard:?}"
    );
    let seen = world.answers.seen();
    assert!(seen[0].contains("since=s5"), "{seen:?}");
    assert_eq!(kept_at(&world).await.as_deref(), Some("s6"));
}

#[tokio::test(start_paused = true)]
async fn a_sync_keeps_where_it_first_stands_and_then_only_where_commands_were_given() {
    let answers = Answers::of(vec![
        answer("sync?timeout=0", 200, &synced_at("s1", &[])),
        answer("since=s1", 200, &synced_at("s2", &[])),
        unsynced("since=s2"),
    ]);
    let world = World::new("matrix", answers, ScriptedSockets::default()).await;
    let heard = world.served(4, |line| line == UNSYNCED).await;
    assert_eq!(heard, [UNSYNCED]);
    assert_eq!(kept_at(&world).await.as_deref(), Some("s1"));
}

#[tokio::test(start_paused = true)]
async fn where_a_sync_stands_that_cannot_be_kept_is_said_before_its_commands_are_taken_up() {
    let answers = Answers::of(vec![
        answer(
            "since=s1",
            200,
            &synced_at("s1", &[("!herald latest", None)]),
        ),
        answer("/send/m.room.message/", 200, "{}"),
        answer(
            "since=s1",
            200,
            &synced_at("s2", &[("!herald changelog", Some(minutes_ago(11)))]),
        ),
        unsynced("since=s2"),
    ]);
    let mut world = World::new("matrix", answers, ScriptedSockets::default()).await;
    world
        .store
        .inner
        .put_position(&matrix(), "s1")
        .await
        .unwrap();
    world.store.fault = Fault::KeepsPositions;
    let answered = Cell::new(false);
    let failed = Cell::new(false);
    let heard = world
        .served(4, |line| {
            answered.set(answered.get() || line.starts_with("answered"));
            failed.set(failed.get() || line == UNSYNCED);
            answered.get() && failed.get()
        })
        .await;
    assert_eq!(
        heard
            .iter()
            .filter(|line| !line.starts_with("answered"))
            .cloned()
            .collect::<Vec<_>>(),
        [
            format!("matrix: where its session stands could not be kept: {}", broken()),
            "matrix: changelog given at 2027-01-15T07:49:00Z was skipped: it was given before 2027-01-15T07:50:00Z, while no session was held".to_owned(),
            UNSYNCED.to_owned(),
        ]
    );
    assert!(
        heard.contains(&"answered latest: Ok(())".to_owned()),
        "{heard:?}"
    );
    assert_eq!(kept_at(&world).await.as_deref(), Some("s1"));
}

#[tokio::test(start_paused = true)]
async fn a_sync_whose_position_cannot_be_read_says_so_and_starts_afresh() {
    let answers = Answers::of(vec![
        answer("sync?timeout=0", 200, &synced_at("s1", &[])),
        unsynced("since=s1"),
    ]);
    let mut world = World::new("matrix", answers, ScriptedSockets::default()).await;
    world
        .store
        .inner
        .put_position(&matrix(), "s9")
        .await
        .unwrap();
    world.store.fault = Fault::ReadsPositions;
    let heard = world.served(4, |line| line == UNSYNCED).await;
    assert_eq!(
        heard,
        [
            format!(
                "matrix: where its session stood could not be read, so it starts afresh: {}",
                broken()
            ),
            UNSYNCED.to_owned(),
        ]
    );
    assert_eq!(kept_at(&world).await.as_deref(), Some("s1"));
}

#[tokio::test(start_paused = true)]
async fn a_sync_on_a_store_keeping_no_positions_starts_afresh_and_keeps_none() {
    let answers = Answers::of(vec![
        answer("sync?timeout=0", 200, &synced_at("s1", &[])),
        answer(
            "since=s1",
            200,
            &synced_at("s2", &[("!herald latest", None)]),
        ),
        answer("/send/m.room.message/", 200, "{}"),
        unsynced("since=s2"),
    ]);
    let mut world = World::new("matrix", answers, ScriptedSockets::default()).await;
    world
        .store
        .inner
        .put_position(&matrix(), "s9")
        .await
        .unwrap();
    world.store.fault = Fault::KeepsNoPositions;
    let answered = Cell::new(false);
    let failed = Cell::new(false);
    let mut heard = world
        .served(4, |line| {
            answered.set(answered.get() || line.starts_with("answered"));
            failed.set(failed.get() || line == UNSYNCED);
            answered.get() && failed.get()
        })
        .await;
    heard.sort();
    assert_eq!(heard, ["answered latest: Ok(())", UNSYNCED]);
    assert_eq!(kept_at(&world).await.as_deref(), Some("s9"));
}
