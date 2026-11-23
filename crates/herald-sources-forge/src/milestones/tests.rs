use std::collections::BTreeMap;

use herald_core::{Validators, Visibility};
use herald_ports::Response;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::SourceOptions;
use crate::options::Milestones;

const PAGE: &str = "https://github.com/o/r";
const CLOSED: i64 = 1_794_999_600;
const UPDATED: i64 = 1_794_996_000;

fn repository() -> Repository {
    Repository {
        id: 7,
        name: "r".to_owned(),
        page: Some(PAGE.to_owned()),
    }
}

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: vec![("etag".to_owned(), format!("\"{status}\""))],
        body: body.to_string().into_bytes(),
    }
}

fn choice(milestones: Milestones) -> MilestoneChoice {
    SourceOptions {
        milestones: Some(milestones),
        ..SourceOptions::default()
    }
    .milestone_choice()
}

fn every() -> MilestoneChoice {
    choice(Milestones {
        actions: Some(vec![
            MilestoneAction::Opened,
            MilestoneAction::Closed,
            MilestoneAction::Reopened,
            MilestoneAction::Deleted,
        ]),
        group: Some(true),
        ..Milestones::default()
    })
}

fn github(number: u64, state: &str, title: &str) -> Value {
    json!({
        "id": 1000 + number, "number": number, "title": title, "description": "Ships the parser.",
        "state": state, "open_issues": 2, "closed_issues": 12,
        "created_at": "2026-11-18T08:00:00Z", "updated_at": "2026-11-18T10:00:00Z",
        "closed_at": (state == "closed").then_some("2026-11-18T11:00:00Z"),
        "due_on": "2026-12-01T08:00:00Z", "html_url": format!("{PAGE}/milestone/{number}"),
    })
}

fn gitlab(id: u64, state: &str, group: bool) -> Value {
    json!({
        "id": id, "iid": id + 100, "title": "v1.4", "description": null, "state": state,
        "project_id": (!group).then_some(7), "group_id": group.then_some(9),
        "due_date": "2026-12-01", "created_at": "2026-11-18T08:00:00Z",
        "updated_at": "2026-11-18T10:00:00Z",
    })
}

fn github_hook(action: &str, milestone: &Value) -> Vec<u8> {
    json!({
        "action": action, "milestone": milestone,
        "repository": { "id": 7, "name": "r", "full_name": "o/r", "html_url": PAGE, "private": true },
    })
    .to_string()
    .into_bytes()
}

fn gitlab_hook(action: &str, milestone: &Value) -> Vec<u8> {
    json!({
        "object_kind": "milestone", "action": action, "object_attributes": milestone,
        "project": { "id": 7, "path_with_namespace": "o/r", "web_url": "https://gitlab.com/o/r", "visibility_level": 0 },
    })
    .to_string()
    .into_bytes()
}

fn identity(event: &Event) -> &str {
    event.identity().as_str()
}

#[test]
fn each_github_action_reads_as_what_the_milestone_did_and_a_deletion_withdraws() {
    let closed = github(3, "closed", "v1.4");
    let cases = [
        (
            "created",
            Some(("github.com/7/milestone/3/opened".to_owned(), false)),
        ),
        (
            "closed",
            Some((format!("github.com/7/milestone/3/closed/{CLOSED}"), false)),
        ),
        (
            "opened",
            Some((
                format!("github.com/7/milestone/3/reopened/{UPDATED}"),
                false,
            )),
        ),
        (
            "deleted",
            Some((format!("github.com/7/milestone/3/closed/{CLOSED}"), true)),
        ),
        ("edited", None),
    ];
    for (action, expected) in cases {
        let found = delivered(Forge::GitHub, &github_hook(action, &closed), &every())
            .unwrap()
            .unwrap();
        assert_eq!(found.path, "o/r");
        let read = found
            .event
            .map(|event| (identity(&event).to_owned(), event.withdrawn()));
        assert_eq!(read, expected, "{action}");
    }
    let open = github(3, "open", "v1.4");
    let deleted = delivered(Forge::GitHub, &github_hook("deleted", &open), &every())
        .unwrap()
        .unwrap()
        .event
        .unwrap();
    assert_eq!(identity(&deleted), "github.com/7/milestone/3/opened");
    let unchosen = delivered(
        Forge::GitHub,
        &github_hook("created", &open),
        &choice(Milestones::default()),
    )
    .unwrap()
    .unwrap();
    assert!(unchosen.event.is_none());
}

#[test]
fn a_milestone_says_its_title_counts_due_day_and_description_and_is_versioned_by_its_title() {
    let event = delivered(
        Forge::GitHub,
        &github_hook("closed", &github(3, "closed", "v2.0-rc.1")),
        &every(),
    )
    .unwrap()
    .unwrap()
    .event
    .unwrap();
    assert_eq!(
        event.title(),
        "Milestone v2.0-rc.1 closed: 12 of 14 closed, due 2026-12-01"
    );
    assert_eq!(event.body(), "Ships the parser.");
    assert_eq!(event.kind(), &Kind::MILESTONE);
    assert_eq!(event.links()[MILESTONE_LINK], format!("{PAGE}/milestone/3"));
    let attributes = event.attributes();
    let text = |value: &str| Some(Attribute::Text(value.to_owned()));
    assert_eq!(
        attributes.get(herald_core::VERSION).cloned(),
        text("v2.0-rc.1")
    );
    assert_eq!(attributes.get(MILESTONE_LINK).cloned(), text("v2.0-rc.1"));
    assert_eq!(attributes.get(DUE).cloned(), text("2026-12-01"));
    assert_eq!(attributes.get(ACTION).cloned(), text("closed"));
    assert_eq!(attributes.get(SIGNAL).cloned(), text("milestone"));
    assert_eq!(
        attributes.get(herald_core::PRERELEASE),
        Some(&Attribute::Flag(true))
    );
    assert_eq!(attributes.get(NUMBER), Some(&Attribute::Number(3.into())));
    assert_eq!(
        attributes.get(ISSUES_OPEN),
        Some(&Attribute::Number(2.into()))
    );
    assert_eq!(
        attributes.get(ISSUES_CLOSED),
        Some(&Attribute::Number(12.into()))
    );
    assert!(event.origin().is_none());
    let uncounted = choice(Milestones {
        counts: Some(false),
        ..Milestones::default()
    });
    let mut bare = github(3, "closed", "Sprint 3");
    bare["due_on"] = Value::Null;
    let event = delivered(Forge::GitHub, &github_hook("closed", &bare), &uncounted)
        .unwrap()
        .unwrap()
        .event
        .unwrap();
    assert_eq!(event.title(), "Milestone Sprint 3 closed");
    for name in [DUE, ISSUES_OPEN, ISSUES_CLOSED] {
        assert!(!event.attributes().contains_key(name), "{name}");
    }
    assert_eq!(
        event.attributes().get(herald_core::PRERELEASE),
        Some(&Attribute::Flag(false))
    );
}

#[test]
fn a_milestone_of_a_repository_named_nothing_makes_no_event() {
    let body = github(3, "closed", "v1.4").to_string();
    let milestone = github::one(Forge::GitHub, "w", 4_096, body.as_bytes()).unwrap();
    let found = ForgeMilestone {
        milestone,
        action: MilestoneAction::Closed,
    };
    let unnamed = Repository {
        name: " ".to_owned(),
        ..repository()
    };
    let error = found.event("w", &unnamed, true).unwrap_err();
    assert!(
        matches!(error, SourceError::Invalid { what: "w", .. }),
        "{error}"
    );
}

#[test]
fn a_description_is_cut_to_four_kibibytes_on_a_character() {
    let mut long = github(3, "closed", "v1.4");
    long["description"] = json!("é".repeat(DESCRIPTION_MOST));
    let event = delivered(Forge::GitHub, &github_hook("closed", &long), &every())
        .unwrap()
        .unwrap()
        .event
        .unwrap();
    assert_eq!(event.body(), "é".repeat(DESCRIPTION_MOST / 2));
    let mut counted = github(3, "closed", "v1.4");
    counted["open_issues"] = json!(u64::from(u32::MAX) + 1);
    let error = delivered(Forge::GitHub, &github_hook("closed", &counted), &every()).unwrap_err();
    assert!(
        error.to_string().contains("GitHub's milestone event"),
        "{error}"
    );
    let unnumbered = json!({
        "action": "closed", "milestone": { "id": 1, "title": "v", "state": "closed", "created_at": "2026-11-18T08:00:00Z" },
        "repository": { "id": 7, "name": "r", "full_name": "o/r" },
    });
    let error = delivered(Forge::GitHub, unnumbered.to_string().as_bytes(), &every()).unwrap_err();
    assert!(error.to_string().contains("names no number"), "{error}");
}

#[test]
fn each_gitlab_action_reads_as_what_the_milestone_did_and_a_group_s_only_where_asked() {
    let cases = [
        ("create", "opened".to_owned()),
        ("close", format!("closed/{UPDATED}")),
        ("reopen", format!("reopened/{UPDATED}")),
        ("delete", format!("closed/{UPDATED}")),
    ];
    for (action, acted) in cases {
        let event = delivered(
            Forge::GitLab,
            &gitlab_hook(action, &gitlab(61, "closed", false)),
            &every(),
        )
        .unwrap()
        .unwrap()
        .event
        .unwrap();
        assert_eq!(
            identity(&event),
            format!("gitlab.com/7/milestone/61/{acted}"),
            "{action}"
        );
        assert_eq!(
            event.links()[MILESTONE_LINK],
            "https://gitlab.com/o/r/-/milestones/161"
        );
        assert_eq!(
            event.attributes().get(NUMBER),
            Some(&Attribute::Number(161.into()))
        );
        assert!(!event.attributes().contains_key(ISSUES_OPEN));
    }
    let updated = delivered(
        Forge::GitLab,
        &gitlab_hook("update", &gitlab(61, "active", false)),
        &every(),
    )
    .unwrap()
    .unwrap();
    assert!(updated.event.is_none());
    let grouped = gitlab_hook("create", &gitlab(62, "active", true));
    let event = delivered(Forge::GitLab, &grouped, &every())
        .unwrap()
        .unwrap()
        .event
        .unwrap();
    assert_eq!(identity(&event), "gitlab.com/group/9/milestone/62/opened");
    let ungrouped = choice(Milestones {
        actions: Some(vec![MilestoneAction::Opened]),
        ..Milestones::default()
    });
    assert!(
        delivered(Forge::GitLab, &grouped, &ungrouped)
            .unwrap()
            .unwrap()
            .event
            .is_none()
    );
    let unprojected =
        json!({ "action": "create", "object_attributes": gitlab(63, "active", true) });
    assert!(
        delivered(Forge::GitLab, unprojected.to_string().as_bytes(), &every())
            .unwrap()
            .is_none()
    );
    let timeless = json!({ "id": 1, "iid": 1, "title": "v", "state": "active" });
    let error = delivered(Forge::GitLab, &gitlab_hook("create", &timeless), &every()).unwrap_err();
    assert!(error.to_string().contains("names no time"), "{error}");
}

#[test]
fn only_github_and_gitlab_send_a_milestone_webhook_and_a_ci_run_reads_github_s() {
    assert!(hooked(Forge::GitHub, "milestone"));
    assert!(hooked(Forge::GitLab, "Milestone Hook"));
    assert!(!hooked(Forge::GitHub, "Milestone Hook"));
    assert!(!hooked(Forge::GitLab, "milestone"));
    assert!(!hooked(Forge::Forgejo, "milestone"));
    assert!(of_a_milestone(Forge::GitHub, "milestone"));
    assert!(!of_a_milestone(Forge::GitHub, "release"));
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    let payload = github_hook("closed", &github(3, "closed", "v1.4"));
    let event = ci_milestone((Forge::GitHub, &options), &payload)
        .unwrap()
        .unwrap();
    let origin = event.origin().unwrap();
    assert_eq!(origin.visibility, Visibility::Private);
    assert_eq!(origin.repository, "o/r");
    assert_eq!(event.bounded_by(), Some(Visibility::Private));
    let opened = github_hook("created", &github(3, "open", "v1.4"));
    assert_eq!(
        ci_milestone((Forge::GitHub, &options), &opened).unwrap(),
        None
    );
    assert!(ci_milestone((Forge::GitHub, &options), b"{}").is_err());
}

struct Polled {
    changed: bool,
    found: Found,
}

fn poll(
    forge: Forge,
    http: &ScriptedHttp,
    cursor: Option<&Cursor>,
    (choice, page): (&MilestoneChoice, Option<usize>),
) -> Result<Polled, Unpolled> {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        page: page.and_then(std::num::NonZeroUsize::new),
        ..SourceOptions::default()
    };
    let origin = Origin::new(forge, &options, "o/r");
    let asking = Asking {
        origin: &origin,
        token: None,
        choice,
    };
    let http = Erased(http as &dyn DynHttp);
    let followed = pollster::block_on(asking.asked(&http, cursor))?;
    let changed = followed.changed;
    let found = asking.read(followed, &repository())?;
    Ok(Polled { changed, found })
}

fn kept(ids: &[(&str, &[&str])]) -> Cursor {
    Cursor {
        ids: ids
            .iter()
            .map(|(key, ids)| {
                (
                    (*key).to_owned(),
                    ids.iter().map(|id| (*id).to_owned()).collect(),
                )
            })
            .collect(),
        listings: BTreeMap::from([(
            "milestone".to_owned(),
            Validators {
                etag: Some("\"old\"".to_owned()),
                last_modified: None,
            },
        )]),
        ..Cursor::default()
    }
}

fn ids(found: &Found) -> BTreeMap<&str, Vec<&str>> {
    found
        .ids
        .iter()
        .map(|(key, ids)| (key.as_str(), ids.iter().map(String::as_str).collect()))
        .collect()
}

#[test]
fn a_first_poll_follows_the_open_milestones_and_announces_none() {
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([github(1, "open", "v1"), github(2, "open", "v2")]),
    )]);
    let polled = poll(Forge::GitHub, &http, None, (&every(), None)).unwrap();
    assert!(polled.changed);
    assert_eq!(polled.found.events, Vec::<Event>::new());
    assert_eq!(
        http.asked(),
        [(
            "https://api.github.com/repos/o/r/milestones?state=open&per_page=30&page=1".to_owned(),
            None
        )]
    );
    assert_eq!(
        ids(&polled.found),
        BTreeMap::from([("milestone", vec!["1", "2"]), ("milestone/closed", vec![])])
    );
    assert_eq!(
        polled.found.listings["milestone"].etag.as_deref(),
        Some("\"200\"")
    );
}

#[test]
fn a_poll_announces_what_appeared_and_reads_once_what_left_the_open_listing() {
    let http = ScriptedHttp::new(vec![
        answer(
            200,
            &json!([
                github(2, "open", "v2"),
                github(5, "open", "v5"),
                github(6, "open", "v6")
            ]),
        ),
        answer(200, &github(1, "closed", "v1")),
        answer(200, &github(3, "open", "v3")),
        answer(404, &json!({ "message": "Not Found" })),
    ]);
    let cursor = kept(&[
        ("milestone", &["1", "2", "3", "4"]),
        ("milestone/closed", &["5", "8"]),
    ]);
    let polled = poll(Forge::GitHub, &http, Some(&cursor), (&every(), None)).unwrap();
    let identities: Vec<&str> = polled.found.events.iter().map(identity).collect();
    assert_eq!(
        identities,
        [
            format!("github.com/7/milestone/5/reopened/{UPDATED}"),
            "github.com/7/milestone/6/opened".to_owned(),
            format!("github.com/7/milestone/1/closed/{CLOSED}"),
        ]
    );
    let asked: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        asked[0],
        "https://api.github.com/repos/o/r/milestones?state=open&per_page=30&page=1"
    );
    assert_eq!(http.asked()[0].1.as_deref(), Some("\"old\""));
    assert_eq!(
        asked[1..],
        [
            "https://api.github.com/repos/o/r/milestones/1",
            "https://api.github.com/repos/o/r/milestones/3",
            "https://api.github.com/repos/o/r/milestones/4",
        ]
    );
    assert_eq!(
        ids(&polled.found),
        BTreeMap::from([
            ("milestone", vec!["2", "5", "6"]),
            ("milestone/closed", vec!["1", "8"])
        ])
    );
    let quiet = ScriptedHttp::new(vec![answer(304, &Value::Null)]);
    let unchanged = poll(Forge::GitHub, &quiet, Some(&cursor), (&every(), None)).unwrap();
    assert!(!unchanged.changed);
    assert_eq!(unchanged.found.events, Vec::<Event>::new());
    assert_eq!(ids(&unchanged.found)["milestone"], ["1", "2", "3", "4"]);
    assert_eq!(
        unchanged.found.listings["milestone"].etag.as_deref(),
        Some("\"old\"")
    );
}

#[test]
fn a_milestone_listed_with_a_closing_reads_as_reopened_and_only_chosen_ones_are_announced() {
    let mut reopened = github(9, "open", "v9");
    reopened["closed_at"] = json!("2026-11-18T09:00:00Z");
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([reopened, github(10, "open", "x10")]),
    )]);
    let titled = choice(Milestones {
        actions: Some(vec![MilestoneAction::Reopened, MilestoneAction::Opened]),
        titles: vec!["v*".to_owned()],
        ..Milestones::default()
    });
    let polled = poll(
        Forge::GitHub,
        &http,
        Some(&kept(&[("milestone", &[])])),
        (&titled, None),
    )
    .unwrap();
    let identities: Vec<&str> = polled.found.events.iter().map(identity).collect();
    assert_eq!(
        identities,
        [format!("github.com/7/milestone/9/reopened/{UPDATED}")]
    );
}

#[test]
fn a_poll_pages_its_listing_and_follows_no_more_than_most() {
    let http = ScriptedHttp::new(vec![
        answer(
            200,
            &json!([github(1, "open", "v1"), github(2, "open", "v2")]),
        ),
        answer(
            200,
            &json!([github(3, "open", "v3"), github(4, "open", "v4")]),
        ),
    ]);
    let most = choice(Milestones {
        most: std::num::NonZeroUsize::new(3),
        ..Milestones::default()
    });
    let polled = poll(Forge::GitHub, &http, None, (&most, Some(2))).unwrap();
    assert_eq!(
        http.asked()[1].0,
        "https://api.github.com/repos/o/r/milestones?state=open&per_page=2&page=2"
    );
    assert_eq!(
        ids(&polled.found),
        BTreeMap::from([
            ("milestone", vec!["1", "2"]),
            ("milestone/2", vec!["3"]),
            ("milestone/closed", vec![])
        ])
    );
    let short = ScriptedHttp::new(vec![answer(200, &json!([github(1, "open", "v1")]))]);
    poll(Forge::GitHub, &short, None, (&most, Some(2))).unwrap();
    assert_eq!(short.asked().len(), 1);
    let unchanged = ScriptedHttp::new(vec![answer(304, &Value::Null), answer(304, &Value::Null)]);
    let cursor = kept(&[("milestone", &["1", "2"]), ("milestone/2", &["3"])]);
    let polled = poll(Forge::GitHub, &unchanged, Some(&cursor), (&most, Some(2))).unwrap();
    assert_eq!(ids(&polled.found)["milestone/2"], ["3"]);
    let two = choice(Milestones {
        most: std::num::NonZeroUsize::new(2),
        ..Milestones::default()
    });
    let small = ScriptedHttp::new(vec![answer(200, &json!([]))]);
    poll(Forge::GitHub, &small, None, (&two, None)).unwrap();
    assert!(
        small.asked()[0]
            .0
            .ends_with("?state=open&per_page=2&page=1")
    );
    let left = ScriptedHttp::new(vec![
        answer(
            200,
            &json!([github(1, "open", "v1"), github(2, "open", "v2")]),
        ),
        answer(200, &json!([])),
        answer(200, &github(3, "closed", "v3")),
    ]);
    let closed = poll(Forge::GitHub, &left, Some(&cursor), (&most, Some(2))).unwrap();
    let identities: Vec<&str> = closed.found.events.iter().map(identity).collect();
    assert_eq!(
        identities,
        [format!("github.com/7/milestone/3/closed/{CLOSED}")]
    );
    assert_eq!(ids(&closed.found)["milestone/2"], Vec::<&str>::new());
}

#[test]
fn a_gitlab_poll_reads_the_group_s_milestones_and_counts_each_announced_one() {
    let statistics = json!({ "statistics": { "counts": { "all": 5, "closed": 4, "opened": 1 } } });
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([])),
        answer(200, &gitlab(1, "closed", false)),
        answer(200, &statistics),
        answer(200, &json!([gitlab(2, "active", true)])),
        answer(200, &statistics),
    ]);
    let cursor = kept(&[("milestone", &["1"]), ("milestone-group", &[])]);
    let polled = poll(Forge::GitLab, &http, Some(&cursor), (&every(), None)).unwrap();
    let asked: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        asked,
        [
            "https://gitlab.com/api/v4/projects/o%2Fr/milestones?state=active&per_page=30&page=1",
            "https://gitlab.com/api/v4/projects/o%2Fr/milestones/1",
            "https://gitlab.com/api/v4/projects/o%2Fr/issues_statistics?milestone=v1.4",
            "https://gitlab.com/api/v4/groups/o/milestones?state=active&per_page=30&page=1",
            "https://gitlab.com/api/v4/groups/o/issues_statistics?milestone=v1.4",
        ]
    );
    let identities: Vec<&str> = polled.found.events.iter().map(identity).collect();
    assert_eq!(
        identities,
        [
            format!("github.com/7/milestone/1/closed/{UPDATED}"),
            "github.com/group/9/milestone/2/opened".to_owned()
        ]
    );
    let title = polled.found.events[0].title();
    assert_eq!(
        title,
        "Milestone v1.4 closed: 4 of 5 closed, due 2026-12-01"
    );
    assert_eq!(
        polled.found.events[0].links()[MILESTONE_LINK],
        format!("{PAGE}/-/milestones/101")
    );
    let uncounted = choice(Milestones {
        counts: Some(false),
        ..Milestones::default()
    });
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([])),
        answer(200, &gitlab(1, "closed", false)),
    ]);
    let polled = poll(Forge::GitLab, &http, Some(&cursor), (&uncounted, None)).unwrap();
    assert_eq!(http.asked().len(), 2);
    assert_eq!(
        polled.found.events[0].title(),
        "Milestone v1.4 closed, due 2026-12-01"
    );
}

#[test]
fn a_forgejo_poll_lists_by_limit_and_links_the_milestone_under_the_repository() {
    let mut milestone = github(4, "closed", "v1.4");
    milestone["html_url"] = Value::Null;
    milestone["number"] = Value::Null;
    milestone["id"] = json!(4);
    let http = ScriptedHttp::new(vec![answer(200, &json!([])), answer(200, &milestone)]);
    let polled = poll(
        Forge::Forgejo,
        &http,
        Some(&kept(&[("milestone", &["4"])])),
        (&every(), None),
    )
    .unwrap();
    assert!(
        http.asked()[0]
            .0
            .ends_with("/repos/o/r/milestones?state=open&limit=30&page=1")
    );
    assert!(http.asked()[1].0.ends_with("/repos/o/r/milestones/4"));
    assert_eq!(
        polled.found.events[0].links()[MILESTONE_LINK],
        format!("{PAGE}/milestone/4")
    );
}

#[test]
fn a_rate_limit_on_any_milestone_read_stops_the_poll_as_one() {
    let limited = Response {
        status: 429,
        headers: vec![("retry-after".to_owned(), "60".to_owned())],
        body: Vec::new(),
    };
    let cursor = kept(&[("milestone", &["1"])]);
    let scripts = [
        (Forge::GitHub, vec![limited.clone()], "GitHub's milestones"),
        (
            Forge::GitHub,
            vec![answer(200, &json!([])), limited.clone()],
            "GitHub's milestone ",
        ),
        (
            Forge::GitLab,
            vec![
                answer(200, &json!([])),
                answer(200, &gitlab(1, "closed", false)),
                limited,
            ],
            "GitLab's issue statistics",
        ),
    ];
    for (forge, answers, what) in scripts {
        let http = ScriptedHttp::new(answers);
        match poll(forge, &http, Some(&cursor), (&every(), None)) {
            Err(Unpolled::RateLimited(reason)) => assert!(reason.starts_with(what), "{reason}"),
            other => panic!("{:?}", other.map(|polled| polled.found.events)),
        }
    }
    let unreadable = [
        (
            Forge::GitHub,
            vec![answer(200, &json!({ "not": "a list" }))],
            "GitHub's milestones",
        ),
        (
            Forge::GitHub,
            vec![
                answer(200, &json!([])),
                answer(200, &json!({ "number": 1 })),
            ],
            "GitHub's milestone",
        ),
        (
            Forge::GitLab,
            vec![
                answer(200, &json!([])),
                answer(200, &gitlab(1, "closed", false)),
                answer(200, &json!({ "statistics": {} })),
            ],
            "GitLab's issue statistics",
        ),
    ];
    for (forge, answers, what) in unreadable {
        let http = ScriptedHttp::new(answers);
        let Err(Unpolled::Unreadable(reason)) = poll(forge, &http, Some(&cursor), (&every(), None))
        else {
            panic!("{what} read");
        };
        assert!(reason.contains(what), "{reason}");
    }
}

#[test]
fn a_group_is_asked_only_of_a_project_a_group_holds() {
    let options = SourceOptions::default();
    let origin = Origin::new(Forge::GitLab, &options, "r");
    assert!(gitlab::group_request(&origin, None, "/milestones").is_err());
    let origin = Origin::new(Forge::GitLab, &options, "a/b/r");
    let request = gitlab::group_request(&origin, None, "/milestones").unwrap();
    assert_eq!(
        request.url.expose(),
        "https://gitlab.com/api/v4/groups/a%2Fb/milestones"
    );
}
