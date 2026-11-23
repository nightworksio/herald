use herald_ports::{Response, Secret};
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::SourceOptions;

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn origin() -> Origin {
    let options = SourceOptions {
        repository: Some("group/app".to_owned()),
        api: Some("https://gitlab.example/api/v4".to_owned()),
        ..SourceOptions::default()
    };
    Origin::new(Forge::GitLab, &options, "group/app")
}

fn car(iid: u64, status: &str, branch: &str) -> Value {
    json!({ "id": iid * 10, "merge_request": { "iid": iid }, "status": status,
        "target_branch": branch, "created_at": "2026-11-18T10:00:00Z" })
}

fn pulled(number: u64) -> Pull {
    let page = json!([{ "iid": number, "title": "t", "web_url": "https://gitlab.example/group/app/-/merge_requests/1",
        "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z" }]);
    let page = super::super::gitlab::page(
        page.to_string().as_bytes(),
        1 << 16,
        ("app", "group/app", None),
    )
    .unwrap();
    page.pulls.into_iter().next().unwrap()
}

fn trains(http: &ScriptedHttp, pulls: &mut [Pull]) -> Result<(), Unpolled> {
    let token = Token::Named(Secret::new("TOKEN", "t"));
    pollster::block_on(trained(
        &Erased(http),
        (&origin(), Some(&token)),
        pulls.iter_mut(),
    ))
}

#[test]
fn each_merge_request_on_a_train_takes_its_place_on_its_target_branch_s_train() {
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            car(1, "merging", "main"),
            car(2, "fresh", "main"),
            car(3, "idle", "release"),
            car(4, "stale", "main"),
        ]),
    )]);
    let mut pulls = [pulled(2), pulled(3), pulled(4), pulled(5)];
    pulls[3].queue = Some(QueueEntry::entered(None));
    trains(&http, &mut pulls).unwrap();
    let read: Vec<Option<(Option<u64>, QueueState)>> = pulls
        .iter()
        .map(|pull| {
            pull.queue
                .as_ref()
                .map(|entry| (entry.position, entry.state))
        })
        .collect();
    assert_eq!(
        read,
        [
            Some((Some(2), QueueState::Checking)),
            Some((Some(1), QueueState::Queued)),
            Some((Some(3), QueueState::Queued)),
            None,
        ]
    );
    assert_eq!(
        pulls[0].queue.as_ref().and_then(|entry| entry.since),
        Some("2026-11-18T10:00:00Z".parse().unwrap())
    );
    assert_eq!(state("merging"), QueueState::Merging);
    let asked: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        asked,
        [
            "https://gitlab.example/api/v4/projects/group%2Fapp/merge_trains?scope=active&sort=asc&per_page=100"
        ]
    );
}

#[test]
fn a_refused_read_is_no_train_unless_it_is_the_rate_limit() {
    let mut pulls = [pulled(2)];
    let http = ScriptedHttp::new(vec![answer(403, &json!({ "message": "403 Forbidden" }))]);
    trains(&http, &mut pulls).unwrap();
    assert_eq!(pulls[0].queue, None);
    let spent = Response {
        status: 403,
        headers: vec![("ratelimit-remaining".to_owned(), "0".to_owned())],
        body: b"{}".to_vec(),
    };
    for limited in [spent, answer(429, &json!({}))] {
        let http = ScriptedHttp::new(vec![limited]);
        assert_eq!(
            trains(&http, &mut pulls),
            Err(Unpolled::RateLimited(
                "the forge's merge train listing was refused for its rate limit; it did not say when the limit resets".to_owned()
            ))
        );
    }
    let http = ScriptedHttp::new(vec![answer(404, &json!({}))]);
    assert!(matches!(
        trains(&http, &mut pulls),
        Err(Unpolled::Unreadable(_))
    ));
    let http = ScriptedHttp::new(vec![answer(200, &json!({ "cars": [] }))]);
    assert!(matches!(
        trains(&http, &mut pulls),
        Err(Unpolled::Unreadable(_))
    ));
    let http = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        trains(&http, &mut pulls),
        Err(Unpolled::Unreachable(_))
    ));
}
