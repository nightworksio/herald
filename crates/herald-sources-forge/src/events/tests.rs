use herald_ports::Response;
use serde_json::{Value, json};

use super::*;
use herald_testing::ScriptedHttp;

fn event(id: u64, kind: &str, action: &str, repo: u64) -> Value {
    json!({ "id": id.to_string(), "type": kind, "repo": { "id": repo }, "payload": { "action": action } })
}

fn answer(events: &[Value], etag: &str) -> Response {
    Response {
        status: 200,
        headers: vec![("ETag".to_owned(), etag.to_owned())],
        body: Value::from(events.to_vec()).to_string().into_bytes(),
    }
}

/// A full page of events, ids `from` down, none a release.
fn full(from: u64) -> Vec<Value> {
    (0..GITHUB_PAGE as u64)
        .map(|offset| event(from - offset, "PushEvent", "", 1))
        .collect()
}

fn changed_by(
    http: &ScriptedHttp,
    owner: Owner,
    last: Option<&EventsRead>,
) -> Result<Changed, Unpolled> {
    changed_with(http, (owner, false), last)
}

fn changed_with(
    http: &ScriptedHttp,
    (owner, tags): (Owner, bool),
    last: Option<&EventsRead>,
) -> Result<Changed, Unpolled> {
    let asking = Asking {
        forge: Forge::GitHub,
        paged: GITHUB_PAGED,
        api: "https://api.github.com",
        name: "lemonfiber",
        owner,
        token: None,
        parse: 1 << 20,
        tags,
    };
    pollster::block_on(changed(&Erased(http as &dyn DynHttp), &asking, last))
}

fn last(newest: u64) -> EventsRead {
    EventsRead {
        etag: Some("\"e1\"".to_owned()),
        newest,
    }
}

#[test]
fn a_first_read_of_an_owner_s_events_is_a_gap_remembering_how_far_it_read() {
    let http = ScriptedHttp::new(vec![answer(&[event(9, RELEASE, PUBLISHED, 3)], "\"e2\"")]);
    assert_eq!(
        changed_by(&http, Owner::Organization, None),
        Ok(Changed::Gap(EventsRead {
            etag: Some("\"e2\"".to_owned()),
            newest: 9
        }))
    );
    assert_eq!(
        http.asked(),
        [(
            "https://api.github.com/orgs/lemonfiber/events?per_page=100&page=1".to_owned(),
            None
        )]
    );
}

#[test]
fn unchanged_events_name_no_member_and_keep_how_far_they_were_read() {
    let http = ScriptedHttp::new(vec![Response {
        status: 304,
        headers: Vec::new(),
        body: Vec::new(),
    }]);
    assert_eq!(
        changed_by(&http, Owner::User, Some(&last(5))),
        Ok(Changed::Named {
            ids: BTreeSet::new(),
            read: last(5)
        })
    );
    assert_eq!(
        http.asked(),
        [(
            "https://api.github.com/users/lemonfiber/events?per_page=100&page=1".to_owned(),
            Some("\"e1\"".to_owned())
        )]
    );
}

#[test]
fn events_reaching_back_name_each_member_a_newer_release_was_published_in() {
    let http = ScriptedHttp::new(vec![answer(
        &[
            event(12, RELEASE, PUBLISHED, 7),
            event(11, RELEASE, "created", 8),
            event(10, "PushEvent", PUBLISHED, 9),
            event(5, RELEASE, PUBLISHED, 4),
        ],
        "\"e2\"",
    )]);
    assert_eq!(
        changed_by(&http, Owner::Organization, Some(&last(5))),
        Ok(Changed::Named {
            ids: BTreeSet::from(["7".to_owned()]),
            read: EventsRead {
                etag: Some("\"e2\"".to_owned()),
                newest: 12
            }
        })
    );
}

#[test]
fn full_pages_are_read_until_one_reaches_back_and_every_page_short_of_it_is_a_gap() {
    let mut second = full(200);
    second[50] = event(150, RELEASE, PUBLISHED, 6);
    let reaching = ScriptedHttp::new(vec![answer(&full(300), "\"e2\""), answer(&second, "")]);
    let Ok(Changed::Named { ids, read }) =
        changed_by(&reaching, Owner::Organization, Some(&last(120)))
    else {
        panic!("the second page reaches back");
    };
    assert_eq!(ids, BTreeSet::from(["6".to_owned()]));
    assert_eq!(read.newest, 300);
    assert_eq!(reaching.asked()[1].1, None);
    assert!(reaching.asked()[1].0.ends_with("&page=2"));
    let short = ScriptedHttp::new(vec![
        answer(&full(500), "\"e2\""),
        answer(&full(400), ""),
        answer(&full(300), ""),
    ]);
    assert!(matches!(
        changed_by(&short, Owner::Organization, Some(&last(10))),
        Ok(Changed::Gap(EventsRead { newest: 500, .. }))
    ));
    assert_eq!(short.asked().len(), GITHUB_PAGES);
    let ended = ScriptedHttp::new(vec![answer(&[event(50, "PushEvent", "", 1)], "\"e2\"")]);
    assert!(matches!(
        changed_by(&ended, Owner::Organization, Some(&last(10))),
        Ok(Changed::Gap(_))
    ));
    let empty = ScriptedHttp::new(vec![answer(&[], "\"e3\"")]);
    assert_eq!(
        changed_by(&empty, Owner::Organization, Some(&last(10))),
        Ok(Changed::Gap(EventsRead {
            etag: Some("\"e3\"".to_owned()),
            newest: 10
        }))
    );
}

#[test]
fn events_that_cannot_be_reached_or_read_fail() {
    let silent = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        changed_by(&silent, Owner::Organization, None),
        Err(Unpolled::Unreachable(_))
    ));
    let refused = ScriptedHttp::new(vec![Response {
        status: 403,
        headers: Vec::new(),
        body: Vec::new(),
    }]);
    assert!(matches!(
        changed_by(&refused, Owner::Organization, None),
        Err(Unpolled::Unreadable(_))
    ));
    let worded = ScriptedHttp::new(vec![answer(
        &[json!({ "id": "x", "type": "PushEvent", "repo": { "id": 1 } })],
        "",
    )]);
    let Err(Unpolled::Unreadable(reason)) = changed_by(&worded, Owner::Organization, None) else {
        panic!("an id that is not a number is unreadable");
    };
    assert!(
        reason.contains("an event's id \"x\" is not a number"),
        "{reason}"
    );
}

#[test]
fn a_tag_made_names_its_member_only_where_a_member_may_announce_tags() {
    let created = |id: u64, ref_type: &str, repo: u64| json!({ "id": id.to_string(), "type": CREATE, "repo": { "id": repo }, "payload": { "ref_type": ref_type } });
    let page = || {
        answer(
            &[
                created(12, TAG, 4),
                created(11, "branch", 5),
                event(10, RELEASE, PUBLISHED, 6),
                event(9, "PushEvent", "", 7),
            ],
            "\"e2\"",
        )
    };
    let named = |tags: bool| {
        let http = ScriptedHttp::new(vec![page()]);
        match changed_with(&http, (Owner::Organization, tags), Some(&last(9))) {
            Ok(Changed::Named { ids, .. }) => ids,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(
        named(true),
        BTreeSet::from(["4".to_owned(), "6".to_owned()])
    );
    assert_eq!(named(false), BTreeSet::from(["6".to_owned()]));
}

fn activity(id: u64, op_type: &str, repo: u64) -> Value {
    json!({ "id": id, "op_type": op_type, "repo_id": repo, "content": "" })
}

fn fed(
    http: &ScriptedHttp,
    (owner, tags): (Owner, bool),
    last: Option<&EventsRead>,
) -> Result<Changed, Unpolled> {
    let asking = Asking {
        forge: Forge::Forgejo,
        paged: (50, 3),
        api: "https://codeberg.org/api/v1",
        name: "forgejo",
        owner,
        token: None,
        parse: 1 << 20,
        tags,
    };
    pollster::block_on(changed(&Erased(http as &dyn DynHttp), &asking, last))
}

#[test]
fn a_forgejo_owner_s_activity_feed_names_the_members_that_published_a_release_or_pushed_a_tag() {
    let feed = [
        activity(12, PUBLISH_RELEASE, 3),
        activity(11, PUSH_TAG, 4),
        activity(10, "commit_repo", 5),
        activity(8, PUBLISH_RELEASE, 6),
    ];
    let http = ScriptedHttp::new(vec![answer(&feed, "\"f2\"")]);
    assert_eq!(
        fed(&http, (Owner::Organization, true), Some(&last(9))),
        Ok(Changed::Named {
            ids: BTreeSet::from(["3".to_owned(), "4".to_owned()]),
            read: EventsRead {
                etag: Some("\"f2\"".to_owned()),
                newest: 12,
            },
        })
    );
    assert_eq!(
        http.asked(),
        [(
            "https://codeberg.org/api/v1/orgs/forgejo/activities/feeds?limit=50&page=1".to_owned(),
            Some("\"e1\"".to_owned())
        )]
    );
    let http = ScriptedHttp::new(vec![answer(&feed, "\"f2\"")]);
    let Ok(Changed::Named { ids, .. }) = fed(&http, (Owner::User, false), Some(&last(9))) else {
        panic!("the feed reaches back");
    };
    assert_eq!(ids, BTreeSet::from(["3".to_owned()]));
    assert_eq!(
        http.asked()[0].0,
        "https://codeberg.org/api/v1/users/forgejo/activities/feeds?limit=50&page=1"
    );
    let full: Vec<Value> = (0..50)
        .map(|offset| activity(200 - offset, "commit_repo", 1))
        .collect();
    let http = ScriptedHttp::new(vec![
        answer(&full, "\"f3\""),
        answer(&[activity(9, PUSH_TAG, 7)], ""),
    ]);
    let Ok(Changed::Named { ids, .. }) = fed(&http, (Owner::Organization, true), Some(&last(9)))
    else {
        panic!("the second page reaches back");
    };
    assert_eq!(ids, BTreeSet::new());
    assert_eq!(http.asked().len(), 2);
    let http = ScriptedHttp::new(vec![answer(&[json!({ "id": "x" })], "")]);
    assert!(matches!(
        fed(&http, (Owner::Organization, true), Some(&last(9))),
        Err(Unpolled::Unreadable(reason)) if reason.starts_with("Forgejo's activity feed")
    ));
}
