//! Who may read a Mastodon route's followers-only statuses, read right
//! before a run's first advisories section is sent there, and no id a
//! server or the ledger gives reaching another path (ADR-0067).

use herald_core::{Entry, Facts, Step};
use herald_dest_fediverse::{Mastodon, MastodonMarkers, RouteOptions, Visibility};
use herald_kinds::{Audience, Reach};
use herald_ports::{Markers, MarkersError, Response, Secret};
use herald_testing::ScriptedHttp;
use herald_testing::sample::{announcement, marker};
use serde_json::{Value, json};

fn mastodon(visibility: Visibility) -> Mastodon {
    Mastodon::new(
        "https://mastodon.example",
        Secret::new("HERALD_MASTODON_TOKEN", "test-token"),
        RouteOptions {
            visibility,
            ..RouteOptions::default()
        },
    )
}

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

#[test]
fn an_edit_on_mastodon_leaves_its_history_readable() {
    assert!(mastodon(Visibility::Private).keeps_edits());
}

fn audience(visibility: Visibility, answers: Vec<Response>) -> (Audience, usize) {
    let http = ScriptedHttp::new(answers);
    let audience = pollster::block_on(mastodon(visibility).audience(&http));
    (audience, http.asked().len())
}

#[test]
fn followers_only_statuses_are_private_only_where_the_account_approves_each_follower() {
    let locked = |locked: bool| vec![answer(200, &json!({ "id": "1", "locked": locked }))];
    assert_eq!(
        audience(Visibility::Private, locked(true)),
        (Audience::Private, 1)
    );
    assert!(matches!(
        audience(Visibility::Private, locked(false)).0,
        Audience::Public(_)
    ));
    for visibility in [Visibility::Public, Visibility::Unlisted] {
        assert!(matches!(
            audience(visibility, Vec::new()),
            (Audience::Public(_), 0)
        ));
    }
    for answers in [
        vec![answer(500, &json!({}))],
        vec![answer(200, &json!({ "id": "1" }))],
        Vec::new(),
    ] {
        assert!(matches!(
            audience(Visibility::Private, answers).0,
            Audience::Unread(_)
        ));
    }
}

#[test]
fn no_status_or_account_id_that_could_reach_another_path_is_used() {
    for id in ["..", ".", "%2e%2e", "1/2", "1?x=2"] {
        let entry = Entry::new(announcement().key).landed(
            Step::Post,
            Facts {
                message: Some(id.to_owned()),
                ..Facts::default()
            },
        );
        let edit = Step::Edit {
            revision: "r2".to_owned(),
            part: 0,
        };
        let refused = mastodon(Visibility::Private)
            .request(&edit, &announcement(), &entry)
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("not an id the platform gives"),
            "{id}: {refused}"
        );
        let account = mastodon(Visibility::Private);
        let http = ScriptedHttp::new(vec![answer(200, &json!({ "id": id }))]);
        let markers = MastodonMarkers::new(&account, &http);
        assert!(
            matches!(
                pollster::block_on(markers.entry(&announcement().key, &marker())),
                Err(MarkersError::Unreadable { .. })
            ),
            "{id}"
        );
        assert_eq!(http.asked().len(), 1, "{id}");
    }
}
