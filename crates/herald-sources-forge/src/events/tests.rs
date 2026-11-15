use herald_ports::Response;
use serde_json::{Value, json};

use super::*;
use crate::scripted::Scripted;

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
    (0..PER_PAGE as u64)
        .map(|offset| event(from - offset, "PushEvent", "", 1))
        .collect()
}

fn changed_by(
    http: &Scripted,
    owner: Owner,
    last: Option<&EventsRead>,
) -> Result<Changed, Unpolled> {
    let asking = Asking {
        api: "https://api.github.com",
        name: "lemonfiber",
        owner,
        token: None,
        parse: 1 << 20,
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
    let http = Scripted::new(vec![answer(&[event(9, RELEASE, PUBLISHED, 3)], "\"e2\"")]);
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
    let http = Scripted::new(vec![Response {
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
    let http = Scripted::new(vec![answer(
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
    let reaching = Scripted::new(vec![answer(&full(300), "\"e2\""), answer(&second, "")]);
    let Ok(Changed::Named { ids, read }) =
        changed_by(&reaching, Owner::Organization, Some(&last(120)))
    else {
        panic!("the second page reaches back");
    };
    assert_eq!(ids, BTreeSet::from(["6".to_owned()]));
    assert_eq!(read.newest, 300);
    assert_eq!(reaching.asked()[1].1, None);
    assert!(reaching.asked()[1].0.ends_with("&page=2"));
    let short = Scripted::new(vec![
        answer(&full(500), "\"e2\""),
        answer(&full(400), ""),
        answer(&full(300), ""),
    ]);
    assert!(matches!(
        changed_by(&short, Owner::Organization, Some(&last(10))),
        Ok(Changed::Gap(EventsRead { newest: 500, .. }))
    ));
    assert_eq!(short.asked().len(), PAGES);
    let ended = Scripted::new(vec![answer(&[event(50, "PushEvent", "", 1)], "\"e2\"")]);
    assert!(matches!(
        changed_by(&ended, Owner::Organization, Some(&last(10))),
        Ok(Changed::Gap(_))
    ));
    let empty = Scripted::new(vec![answer(&[], "\"e3\"")]);
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
    let silent = Scripted::new(Vec::new());
    assert!(matches!(
        changed_by(&silent, Owner::Organization, None),
        Err(Unpolled::Unreachable(_))
    ));
    let refused = Scripted::new(vec![Response {
        status: 403,
        headers: Vec::new(),
        body: Vec::new(),
    }]);
    assert!(matches!(
        changed_by(&refused, Owner::Organization, None),
        Err(Unpolled::Unreadable(_))
    ));
    let worded = Scripted::new(vec![answer(
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
