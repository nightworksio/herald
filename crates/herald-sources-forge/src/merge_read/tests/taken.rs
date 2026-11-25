use herald_kinds::Holds;

use super::*;

type Read<T> = (ScriptedHttp, Result<T, Unpolled>);

fn asked(enter: &Enter) -> EnterAsked<'_> {
    EnterAsked {
        enter,
        least: Permission::Write,
        vouches: Vouches::Gesture,
        skew: SKEW,
        updated: herald_core::merging::Updated::Signed,
    }
}

fn taken(forge: Forge, answers: Vec<Response>, enter: &Enter) -> Read<Vec<TakenBack>> {
    let http = ScriptedHttp::new(answers);
    let taken = pollster::block_on(reader(forge).taken_back(&http, "main", asked(enter)));
    (http, taken)
}

fn took(forge: Forge, answers: Vec<Response>, enter: &Enter) -> Read<Option<TakenBack>> {
    let http = ScriptedHttp::new(answers);
    let took = pollster::block_on(reader(forge).took_back(&http, ("main", 7), asked(enter)));
    (http, took)
}

fn github_queue(numbers: &[u64], total: usize) -> Value {
    let nodes: Vec<Value> = numbers
        .iter()
        .map(|number| json!({ "pullRequest": { "number": number } }))
        .collect();
    json!({ "data": { "repository": { "mergeQueue": { "entries": { "totalCount": total, "nodes": nodes } } } } })
}

fn label_event(event: &str, login: Option<&str>, minute: u32) -> Value {
    json!({ "id": minute, "event": event, "actor": login.map(|login| json!({ "login": login, "id": 2 })),
        "created_at": stamp(minute), "label": { "name": LABEL } })
}

fn pushes(push: bool) -> Value {
    json!({ "user": { "permissions": { "push": push, "pull": true } } })
}

fn made(id: u64, by: &str, minute: u32) -> Marked {
    Marked {
        id: Some(id),
        by: by.to_owned(),
        at: at(minute),
    }
}

fn github_taken(cancel: Marked, held: Option<Holds>) -> TakenBack {
    let mut makers = BTreeMap::from([("w".to_owned(), Holds::Role(Permission::Write))]);
    makers.extend(held.map(|held| (cancel.by.clone(), held)));
    TakenBack {
        number: 7,
        url: "https://github.com/o/r/pull/7".to_owned(),
        id: Some("PR_7".to_owned()),
        gestures: vec![made(11, "w", 11), made(10, "w", 10)],
        cancels: vec![cancel],
        makers,
        whole: true,
    }
}

fn github_answers(cancel_by: Option<&str>) -> Vec<Response> {
    vec![
        ok(&github_pull("open", "main", &[])),
        ok(&json!([
            label_event("labeled", Some("w"), 10),
            label_event("labeled", Some("w"), 11),
            label_event("unlabeled", cancel_by, 12),
            label_event("labeled", Some("w"), 13)
        ])),
        ok(&pushes(true)),
    ]
}

#[test]
fn a_pull_request_in_github_s_merge_queue_is_read_with_every_mark_newest_first() {
    let label = Enter::Label(LABEL.to_owned());
    let mut answers = vec![ok(&github_queue(&[7, 7], 2))];
    answers.extend(github_answers(Some("x")));
    answers.push(ok(&pushes(false)));
    let (http, read) = taken(Forge::GitHub, answers, &label);
    let mut expected = github_taken(made(12, "x", 12), Some(Holds::Nothing));
    expected.gestures.insert(0, made(13, "w", 13));
    assert_eq!(
        read.unwrap(),
        [expected],
        "a gesture made again after the cancel"
    );
    assert_eq!(
        http.asked()
            .into_iter()
            .map(|(url, _)| url)
            .collect::<Vec<_>>(),
        [
            "https://api.github.com/graphql",
            "https://api.github.com/repos/o/r/pulls/7",
            "https://api.github.com/repos/o/r/issues/7/events?per_page=100&page=1",
            "https://api.github.com/repos/o/r/collaborators/w/permission",
            "https://api.github.com/repos/o/r/collaborators/x/permission",
        ]
    );
    assert_eq!(http.bodies()[0]["variables"]["branch"], "main");
    let mut pull = github_answers(Some("x"));
    pull[1] = ok(&json!([
        label_event("unlabeled", Some("x"), 12),
        label_event("labeled", Some("w"), 10),
        label_event("labeled", Some("w"), 11)
    ]));
    pull.push(ok(&pushes(true)));
    let (_, read) = took(Forge::GitHub, pull, &label);
    assert_eq!(
        read.unwrap(),
        Some(github_taken(
            made(12, "x", 12),
            Some(Holds::Role(Permission::Write))
        )),
        "listed out of order"
    );
    let mut cancelled = github_answers(Some("x"));
    cancelled[1] = ok(&json!([label_event("unlabeled", Some("x"), 12)]));
    let (http, read) = took(Forge::GitHub, cancelled, &label);
    assert_eq!(read.unwrap(), None, "no gesture made on it");
    assert_eq!(http.requests().len(), 2);
}

#[test]
fn a_label_taken_off_by_a_deleted_account_asks_no_role() {
    let label = Enter::Label(LABEL.to_owned());
    let mut pull = github_answers(None);
    pull[1] = ok(&json!([
        label_event("labeled", Some("w"), 10),
        label_event("labeled", Some("w"), 11),
        label_event("unlabeled", None, 12)
    ]));
    let (http, read) = took(Forge::GitHub, pull, &label);
    assert_eq!(read.unwrap(), Some(github_taken(made(12, "", 12), None)));
    assert_eq!(http.requests().len(), 3);
}

#[test]
fn a_pull_request_no_longer_open_into_the_branch_is_not_taken_back() {
    let label = Enter::Label(LABEL.to_owned());
    let (http, gone) = took(Forge::GitHub, vec![refused(404)], &label);
    assert_eq!(gone.unwrap(), None);
    assert_eq!(http.requests().len(), 1);
    let answers = vec![
        ok(&github_queue(&[7], 1)),
        ok(&github_pull("closed", "main", &[])),
    ];
    let (http, read) = taken(Forge::GitHub, answers, &label);
    assert_eq!(read.unwrap(), []);
    assert_eq!(http.requests().len(), 2);
}

#[test]
fn github_s_queue_read_in_part_or_past_herald_s_reach_stops_the_read() {
    let label = Enter::Label(LABEL.to_owned());
    let (_, partial) = taken(Forge::GitHub, vec![ok(&github_queue(&[7], 2))], &label);
    assert!(
        matches!(partial, Err(Unpolled::Unreadable(ref why)) if why.contains("listed 1 of the 2")),
        "{partial:?}"
    );
    let numbers: Vec<u64> = (1..=MOST_ENTERED as u64 + 1).collect();
    let (http, beyond) = taken(
        Forge::GitHub,
        vec![ok(&github_queue(&numbers, numbers.len()))],
        &label,
    );
    assert!(
        matches!(beyond, Err(Unpolled::Unreadable(ref why)) if why.contains(&format!("more than {MOST_ENTERED}"))),
        "{beyond:?}"
    );
    assert_eq!(http.requests().len(), 1);
    let numbers: Vec<u64> = (1..=MOST_ENTERED as u64).chain([1]).collect();
    let mut answers = vec![ok(&github_queue(&numbers, numbers.len()))];
    answers.extend((0..MOST_ENTERED).map(|_| refused(404)));
    let (http, within) = taken(Forge::GitHub, answers, &label);
    assert_eq!(within.unwrap(), []);
    assert_eq!(http.requests().len(), MOST_ENTERED + 1);
    let unqueued = json!({ "data": { "repository": { "mergeQueue": null } } });
    let (http, none) = taken(Forge::GitHub, vec![ok(&unqueued)], &label);
    assert_eq!(none.unwrap(), []);
    assert_eq!(http.requests().len(), 1);
    let (http, forgejo) = taken(Forge::Forgejo, Vec::new(), &label);
    assert_eq!(forgejo.unwrap(), []);
    assert_eq!(http.requests().len(), 0);
}

fn merge(iid: u64, state: &str, set: bool) -> Value {
    json!({ "iid": iid, "web_url": format!("u{iid}"), "sha": "h7", "state": state, "target_branch": "main",
        "author": { "username": "r", "id": 7 }, "merge_when_pipeline_succeeds": set })
}

fn note(id: u64, (username, user): (&str, u64), body: &str, minute: u32) -> Value {
    json!({ "id": id, "author": { "id": user, "username": username }, "body": body,
        "created_at": stamp(minute), "updated_at": stamp(minute), "system": false })
}

#[test]
fn a_cancel_on_a_merge_request_on_gitlab_s_train_or_set_to_join_it_takes_it_back() {
    let comment = Enter::Comment("/merge".to_owned());
    let answers = vec![
        ok(&json!([{ "merge_request": { "iid": 7 } }, { "merge_request": null }])),
        ok(&json!([
            merge(8, "opened", true),
            merge(9, "opened", false)
        ])),
        ok(&merge(7, "opened", false)),
        ok(&json!([
            note(10, ("w", 2), "/merge", 10),
            note(12, ("r", 7), "/merge cancel", 12)
        ])),
        ok(&json!({ "access_level": 30 })),
        refused(404),
        ok(&merge(8, "merged", true)),
    ];
    let (http, read) = taken(Forge::GitLab, answers, &comment);
    assert_eq!(
        read.unwrap(),
        [TakenBack {
            number: 7,
            url: "u7".to_owned(),
            id: None,
            gestures: vec![made(10, "w", 10)],
            cancels: vec![made(12, "r", 12)],
            makers: BTreeMap::from([
                ("w".to_owned(), Holds::Role(Permission::Write)),
                ("r".to_owned(), Holds::Nothing),
            ]),
            whole: true,
        }]
    );
    let asked: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        asked[..2],
        [
            "https://gitlab.com/api/v4/projects/o%2Fr/merge_trains/main?scope=active&per_page=100&page=1",
            "https://gitlab.com/api/v4/projects/o%2Fr/merge_requests?state=opened&target_branch=main&per_page=100&page=1",
        ]
    );
    assert_eq!(
        asked[4..6],
        [
            "https://gitlab.com/api/v4/projects/o%2Fr/members/all/2",
            "https://gitlab.com/api/v4/projects/o%2Fr/members/all/7",
        ]
    );
    assert_eq!(asked.len(), 7);
}

#[test]
fn gitlab_s_train_longer_than_herald_reads_stops_the_read() {
    let comment = Enter::Comment("/merge".to_owned());
    let car = |index: usize| json!({ "merge_request": { "iid": index } });
    let (_, long) = taken(Forge::GitLab, pages(5, 100, car), &comment);
    assert!(
        matches!(long, Err(Unpolled::Unreadable(ref why)) if why.contains("merge train")),
        "{long:?}"
    );
}
