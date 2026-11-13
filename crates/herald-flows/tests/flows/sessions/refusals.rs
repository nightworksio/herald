//! Sessions that cannot take a command up, list its completions or use their credential: each says so, and answers the member where it can.

use super::*;

#[tokio::test(start_paused = true)]
async fn a_command_with_no_place_is_answered_as_unavailable_and_said() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![
        hello(60_000),
        interaction(2, &json!({ "name": "latest" })),
    ])]);
    let answers = Answers::of(vec![
        answer(&format!("/interactions/900/{TOKEN}/callback"), 204, ""),
        answer(
            &format!("/webhooks/{APPLICATION}/{TOKEN}/messages/@original"),
            200,
            "{}",
        ),
    ]);
    let world = World::new("discord", answers, sockets).await;
    let heard = world
        .served(0, |line| line.contains("was not answered"))
        .await;
    assert_eq!(
        heard,
        ["discord: latest was not answered: no more is answered at once"]
    );
    let seen = world.answers.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(seen[0].ends_with("/callback"), "{seen:?}");
    assert!(seen[1].ends_with("/messages/@original"), "{seen:?}");
    let bodies = world.answers.bodies();
    assert_eq!(bodies[0], r#"{"data":{"flags":64},"type":5}"#);
    let replied: Value = serde_json::from_str(&bodies[1]).unwrap();
    assert_eq!(
        replied["content"],
        r"herald could not answer just now\; try again later\."
    );
}

#[tokio::test(start_paused = true)]
async fn a_command_with_no_place_whose_asker_cannot_be_told_says_both() {
    let callback = format!("/interactions/900/{TOKEN}/callback");
    for (script, unsent) in [
        (
            Vec::new(),
            unscripted("POST", &format!("https://discord.com/api/v10{callback}")),
        ),
        (
            vec![answer(&callback, 204, "")],
            unscripted(
                "PATCH",
                &format!(
                    "https://discord.com/api/v10/webhooks/{APPLICATION}/{TOKEN}/messages/@original"
                ),
            ),
        ),
    ] {
        let sockets = ScriptedSockets::opening(vec![Ok(vec![
            hello(60_000),
            interaction(2, &json!({ "name": "latest" })),
        ])]);
        let world = World::new("discord", Answers::of(script), sockets).await;
        let heard = world
            .served(0, |line| line.contains("was not answered"))
            .await;
        assert_eq!(
            heard,
            [format!(
                "discord: latest was not answered: no more is answered at once, and the asker could not be told: {unsent}"
            )]
        );
    }
}

/// Why a request nothing was scripted for was not sent.
fn unscripted(method: &str, url: &str) -> String {
    format!("the host could not be reached: nothing scripted for {method} {url}")
}

#[tokio::test(start_paused = true)]
async fn a_completion_whose_projects_cannot_be_listed_is_said_and_nothing_sent() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![
        hello(60_000),
        interaction(4, &json!({ "name": "latest", "options": [] })),
    ])]);
    let mut world = World::new("discord", Answers::default(), sockets).await;
    world.store.fault = Fault::ListsLatest;
    let heard = world
        .served(4, |line| line.contains("could not be offered"))
        .await;
    assert_eq!(
        heard,
        [format!(
            "discord: completions could not be offered: the projects could not be listed: {}",
            broken()
        )]
    );
    assert_eq!(world.answers.seen(), [] as [String; 0]);
}

#[tokio::test(start_paused = true)]
async fn a_sync_whose_credential_is_refused_stops_and_is_not_tried_again() {
    for status in [401, 403] {
        let answers = Answers::of(vec![answer(
            "sync?timeout=0",
            status,
            r#"{"errcode": "M_UNKNOWN_TOKEN"}"#,
        )]);
        let world = World::new("matrix", answers, ScriptedSockets::default()).await;
        let heard = world.served(4, |line| line == ENDED).await;
        assert_eq!(
            heard,
            [
                format!(
                    "matrix: the homeserver could not be synced: the credential was refused, answering {status}, so the session stops"
                ),
                ENDED.to_owned(),
            ]
        );
        assert_eq!(world.answers.seen().len(), 1);
    }
}

#[tokio::test(start_paused = true)]
async fn socket_mode_whose_credential_is_refused_stops_and_is_not_opened_again() {
    for status in [401, 403] {
        let answers = Answers::of(vec![answer("/apps.connections.open", status, "{}")]);
        let sockets = ScriptedSockets::default();
        let world = World::new("slack", answers, sockets.clone()).await;
        let heard = world.served(4, |line| line == ENDED).await;
        assert_eq!(
            heard,
            [
                format!(
                    "slack: Slack's Socket Mode could not be opened: the credential was refused, answering {status}, so the session stops"
                ),
                ENDED.to_owned(),
            ]
        );
        assert_eq!(world.answers.seen().len(), 1);
        assert_eq!(sockets.opened(), [] as [String; 0]);
    }
}
