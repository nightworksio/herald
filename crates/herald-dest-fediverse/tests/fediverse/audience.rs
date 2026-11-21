//! Who may read a Mastodon route's followers-only statuses, read right
//! before a run's first advisories section is sent there, and no id a
//! server or the ledger gives reaching another path (ADR-0067).

use herald_core::{Entry, Facts, Step};
use herald_dest_fediverse::{Mastodon, MastodonMarkers, RouteOptions, Visibility};
use herald_kinds::{Audience, DestinationConfig as _, Reach};
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

#[test]
fn an_account_the_server_would_not_describe_is_unread_whatever_its_answer_holds() {
    let (refused, asked) = audience(
        Visibility::Private,
        vec![answer(403, &json!({ "id": "1", "locked": true }))],
    );
    assert_eq!(
        refused,
        Audience::Unread(
            "the server would not say whether the account approves its followers, as it answered 403"
                .to_owned()
        )
    );
    assert_eq!(asked, 1);
}

#[test]
fn a_route_is_public_unless_it_posts_to_its_followers_and_the_actor_always_is() {
    let mastodon: herald_dest_fediverse::DestinationOptions = serde_json::from_value(json!({
        "server": "https://mastodon.example",
        "token": { "secret": "HERALD_MASTODON_TOKEN" },
    }))
    .unwrap();
    let placed = |visibility: Visibility| {
        herald_kinds::Placed::new(RouteOptions {
            visibility,
            ..RouteOptions::default()
        })
    };
    let delivery = herald_core::Delivery::default();
    let open = Some(
        "its statuses are posted \"public\" or \"unlisted\", which anyone may read; set visibility = \"private\" to post to its followers alone",
    );
    assert_eq!(
        mastodon.public(&placed(Visibility::Private), &delivery),
        None
    );
    assert_eq!(
        mastodon.public(&placed(Visibility::Public), &delivery),
        open
    );
    assert_eq!(
        mastodon.public(&placed(Visibility::Unlisted), &delivery),
        open
    );
    let actor: herald_dest_fediverse::ActorOptions = serde_json::from_value(json!({
        "domain": "herald.example",
        "key": { "secret": "HERALD_ACTIVITYPUB_KEY" },
    }))
    .unwrap();
    assert_eq!(
        actor.public(&placed(Visibility::Private), &delivery),
        Some("herald's own ActivityPub actor serves its Notes to anyone who looks")
    );
}

#[test]
fn the_actor_declines_nothing_and_is_never_shown_a_section_and_mastodon_declines_its_errors() {
    let actor = herald_dest_fediverse::ActivityPub::new(
        "herald.example",
        "",
        Secret::new("HERALD_ACTIVITYPUB_KEY", "the-key"),
        herald_dest_fediverse::ActorRouteOptions {
            actor: "gate".to_owned(),
            content_warning: None,
            visibility: herald_dest_fediverse::Audience::Public,
            language: None,
        },
    );
    let refusal = answer(403, &json!({ "error": "forbidden" }));
    assert!(!actor.declines(&refusal));
    let http = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        pollster::block_on(actor.audience(&http)),
        Audience::Public(_)
    ));
    assert_eq!(http.asked().len(), 0);
    let account = mastodon(Visibility::Private);
    assert!(account.declines(&refusal));
    assert!(!account.declines(&answer(500, &json!({ "message": "down" }))));
}

#[test]
fn a_status_id_of_letters_digits_dashes_and_underscores_is_edited() {
    let entry = Entry::new(announcement().key).landed(
        Step::Post,
        Facts {
            message: Some("a-B_9".to_owned()),
            ..Facts::default()
        },
    );
    let edit = Step::Edit {
        revision: "r2".to_owned(),
        part: 0,
    };
    let request = mastodon(Visibility::Private)
        .request(&edit, &announcement(), &entry)
        .unwrap();
    assert_eq!(
        request.url.expose(),
        "https://mastodon.example/api/v1/statuses/a-B_9"
    );
}
