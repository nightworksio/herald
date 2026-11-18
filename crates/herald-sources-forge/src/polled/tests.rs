use std::collections::{BTreeMap, BTreeSet};

use herald_core::{PrereleaseRule, Validators};
use serde_json::{Value, json};

use super::*;
use crate::SourceOptions;
use herald_testing::ScriptedHttp;

/// The round each poll here is made in.
const ROUND: jiff::Timestamp = jiff::Timestamp::UNIX_EPOCH;

fn answer(status: u16, body: &Value, etag: Option<&str>) -> Response {
    Response {
        status,
        headers: etag
            .map(|etag| vec![("ETag".to_owned(), etag.to_owned())])
            .unwrap_or_default(),
        body: body.to_string().into_bytes(),
    }
}

fn unchanged() -> Response {
    answer(304, &json!(null), None)
}

fn repository(forge: Forge) -> Response {
    let page = match forge {
        Forge::GitHub | Forge::Forgejo => {
            json!({"id": 7, "name": "r", "html_url": "https://forge.example/o/r"})
        }
        Forge::GitLab => json!({"id": 7, "name": "r", "web_url": "https://forge.example/o/r"}),
    };
    answer(200, &page, None)
}

fn release(tag: &str, published: &str) -> Value {
    json!({
        "tag_name": tag,
        "name": null,
        "body": "notes",
        "html_url": format!("https://forge.example/o/r/releases/tag/{tag}"),
        "draft": false,
        "prerelease": false,
        "published_at": published
    })
}

fn releases(
    forge: Forge,
    signals: &[Signal],
    (prerelease, backfill): (PrereleaseRule, usize),
) -> Releases {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        api: Some("https://forge.example/api".to_owned()),
        ..SourceOptions::default()
    };
    Releases {
        origin: Origin::new(forge, &options, "o/r"),
        token: None,
        project: None,
        signals: Signals {
            taken: signals.iter().copied().collect::<BTreeSet<_>>(),
            prerelease,
        },
        backfill,
        held: None,
        watched: 0,
        packaging: Packaging::of(forge, &options),
        list: crate::packages::PackageList::Own,
        visibility: None,
        runs: crate::options::RunChoice::default(),
        alerts: crate::SourceOptions::default().alert_choice(crate::Forge::GitHub),
    }
}

fn polled(
    releases: &Releases,
    http: &ScriptedHttp,
    cursor: Option<&Cursor>,
) -> Result<Option<Polled>, Unpolled> {
    pollster::block_on(releases.polled(http, cursor, ROUND))
}

fn identities(polled: &Polled) -> Vec<(&str, Option<Signal>)> {
    polled
        .events
        .iter()
        .map(|event| (event.identity().as_str(), event.signal()))
        .collect()
}

fn seen(identities: &[&str]) -> Cursor {
    Cursor {
        seen: identities.iter().map(|seen| Identity::new(*seen)).collect(),
        ..Cursor::default()
    }
}

#[test]
fn a_repository_answer_that_names_no_origin_fails_the_poll_and_the_tag() {
    let unreadable = || {
        answer(
            200,
            &json!({"id": 7, "name": "r", "html_url": "https://forge.example/o/r", "topics": 3}),
            None,
        )
    };
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([release("v1", "2026-11-01T10:00:00Z")]), None),
        unreadable(),
    ]);
    let source = releases(
        Forge::GitHub,
        &[Signal::Release],
        (PrereleaseRule::Either, 0),
    );
    assert!(matches!(
        polled(&source, &http, None),
        Err(Unpolled::Unreadable(_))
    ));
    let tagged = ScriptedHttp::new(vec![unreadable()]);
    assert!(matches!(
        one_tag(Forge::GitHub, &tagged),
        Err(Unpolled::Unreadable(_))
    ));
}

#[test]
fn releases_alone_ask_no_tags() {
    let http = ScriptedHttp::new(vec![
        answer(
            200,
            &json!([release("v1", "2026-11-01T10:00:00Z")]),
            Some("r1"),
        ),
        repository(Forge::GitHub),
    ]);
    let source = releases(
        Forge::GitHub,
        &[Signal::Release],
        (PrereleaseRule::Either, 0),
    );
    let source = Releases {
        watched: 2,
        ..source
    };
    let found = polled(&source, &http, None).unwrap().unwrap();
    assert_eq!(
        identities(&found),
        [("forge.example/7/v1", Some(Signal::Release))]
    );
    assert_eq!(found.watched, 2);
    let origin = found.events[0].origin().unwrap();
    assert_eq!(
        (
            origin.forge.as_str(),
            origin.host.as_str(),
            origin.repository.as_str(),
            origin.id.as_str()
        ),
        ("github", "forge.example", "o/r", "7")
    );
    assert_eq!(found.etag.as_deref(), Some("r1"));
    assert!(found.signals.is_empty());
    assert_eq!(
        http.asked(),
        [
            (
                "https://forge.example/api/repos/o/r/releases?per_page=30".to_owned(),
                None
            ),
            ("https://forge.example/api/repos/o/r".to_owned(), None),
        ]
    );
}

#[test]
fn a_tag_with_a_release_is_the_release_s_and_a_tag_without_one_is_read_in_full() {
    let http = ScriptedHttp::new(vec![
        answer(
            200,
            &json!([release("v1", "2026-11-01T10:00:00Z")]),
            Some("r1"),
        ),
        answer(
            200,
            &json!([{"name": "v2-rc.1", "commit": {"sha": "c2"}}, {"name": "v1", "commit": {"sha": "c1"}}]),
            Some("t1"),
        ),
        repository(Forge::GitHub),
        answer(200, &json!({"object": {"type": "tag", "sha": "o2"}}), None),
        answer(
            200,
            &json!({"tag": "renamed", "message": "Candidate", "tagger": {"date": "2026-11-02T10:00:00Z"}}),
            None,
        ),
    ]);
    let source = releases(
        Forge::GitHub,
        &[Signal::Release, Signal::Tag],
        (PrereleaseRule::Either, 0),
    );
    let found = polled(&source, &http, Some(&seen(&[]))).unwrap().unwrap();
    assert_eq!(
        identities(&found),
        [
            ("forge.example/7/v2-rc.1", Some(Signal::Tag)),
            ("forge.example/7/v1", Some(Signal::Release)),
        ]
    );
    let tag = &found.events[0];
    assert_eq!(tag.version(), Some("v2-rc.1"));
    assert_eq!(tag.body(), "Candidate");
    assert_eq!(tag.title(), "v2-rc.1");
    assert!(tag.prerelease());
    assert_eq!(
        tag.link("release"),
        Some("https://forge.example/o/r/releases/tag/v2-rc.1")
    );
    assert_eq!(tag.occurred().to_string(), "2026-11-02T10:00:00Z");
    assert_eq!(
        found.signals.get(&Signal::Tag),
        Some(&Validators {
            etag: Some("t1".to_owned()),
            last_modified: None
        })
    );
    let asked: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        asked[1..],
        [
            "https://forge.example/api/repos/o/r/tags?per_page=30",
            "https://forge.example/api/repos/o/r",
            "https://forge.example/api/repos/o/r/git/ref/tags/v2-rc.1",
            "https://forge.example/api/repos/o/r/git/tags/o2",
        ]
    );
}

#[test]
fn a_lightweight_github_tag_takes_its_commit_s_date() {
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([{"name": "v3", "commit": {"sha": "c3"}}]), None),
        repository(Forge::GitHub),
        answer(
            200,
            &json!({"object": {"type": "commit", "sha": "c3"}}),
            None,
        ),
        answer(
            200,
            &json!({"committer": {"date": "2026-11-05T10:00:00Z"}}),
            None,
        ),
    ]);
    let source = releases(Forge::GitHub, &[Signal::Tag], (PrereleaseRule::Flag, 0));
    let found = polled(&source, &http, Some(&seen(&[]))).unwrap().unwrap();
    assert_eq!(
        identities(&found),
        [("forge.example/7/v3", Some(Signal::Tag))]
    );
    assert_eq!(found.events[0].body(), "");
    assert_eq!(
        found.events[0].occurred().to_string(),
        "2026-11-05T10:00:00Z"
    );
    assert_eq!(found.etag, None);
    assert_eq!(
        http.asked().last().map(|(url, _)| url.clone()),
        Some("https://forge.example/api/repos/o/r/git/commits/c3".to_owned())
    );
}

#[test]
fn with_no_cursor_only_the_newest_backfill_tags_are_read_and_the_rest_remembered() {
    let http = ScriptedHttp::new(vec![
        answer(
            200,
            &json!([
                {"name": "v3", "message": "", "created_at": null, "commit": {"committed_date": "2026-11-03T10:00:00Z"}},
                {"name": "v2", "message": "", "created_at": null, "commit": {"committed_date": "2026-11-02T10:00:00Z"}},
                {"name": "v1", "message": "", "created_at": null, "commit": {"committed_date": "2026-11-01T10:00:00Z"}}
            ]),
            None,
        ),
        repository(Forge::GitLab),
    ]);
    let source = releases(Forge::GitLab, &[Signal::Tag], (PrereleaseRule::Tag, 1));
    let found = polled(&source, &http, None).unwrap().unwrap();
    assert_eq!(
        identities(&found),
        [("forge.example/7/v3", Some(Signal::Tag))]
    );
    assert_eq!(
        found.passed,
        [
            Identity::new("forge.example/7/v2"),
            Identity::new("forge.example/7/v1")
        ]
    );
    assert_eq!(
        found.events[0].link("release"),
        Some("https://forge.example/o/r/-/tags/v3")
    );
    assert_eq!(
        http.asked()[0].0,
        "https://forge.example/api/projects/o%2Fr/repository/tags?order_by=updated&sort=desc&per_page=30"
    );
}

#[test]
fn a_seen_tag_is_left_out_and_a_forgejo_annotated_tag_reads_its_object() {
    let http = ScriptedHttp::new(vec![
        answer(
            200,
            &json!([
                {"name": "v2", "message": "Two", "id": "obj", "commit": {"sha": "c2", "created": "2026-11-01T10:00:00Z"}},
                {"name": "v1", "message": "", "id": "c1", "commit": {"sha": "c1", "created": "2026-10-01T10:00:00Z"}}
            ]),
            None,
        ),
        repository(Forge::Forgejo),
        answer(
            200,
            &json!({"tag": "renamed", "message": "", "tagger": {"date": "2026-11-02T10:00:00Z"}}),
            None,
        ),
    ]);
    let source = releases(Forge::Forgejo, &[Signal::Tag], (PrereleaseRule::Either, 0));
    let found = polled(&source, &http, Some(&seen(&["forge.example/7/v1"])))
        .unwrap()
        .unwrap();
    assert_eq!(
        identities(&found),
        [("forge.example/7/v2", Some(Signal::Tag))]
    );
    assert_eq!(found.events[0].body(), "Two");
    assert_eq!(
        found.events[0].occurred().to_string(),
        "2026-11-02T10:00:00Z"
    );
    assert_eq!(
        http.asked()[0].0,
        "https://forge.example/api/repos/o/r/tags?limit=30"
    );
    assert_eq!(
        http.asked()[2].0,
        "https://forge.example/api/repos/o/r/git/tags/obj"
    );
}

#[test]
fn each_signal_is_asked_with_its_own_validators_and_keeps_them_when_unchanged() {
    let cursor = Cursor {
        etag: Some("r1".to_owned()),
        last_modified: Some("Sun, 15 Nov 2026 10:00:00 GMT".to_owned()),
        signals: BTreeMap::from([(
            Signal::Tag,
            Validators {
                etag: Some("t1".to_owned()),
                last_modified: None,
            },
        )]),
        ..Cursor::default()
    };
    let source = releases(
        Forge::GitHub,
        &[Signal::Release, Signal::Tag],
        (PrereleaseRule::Either, 0),
    );
    let quiet = ScriptedHttp::new(vec![unchanged(), unchanged()]);
    assert_eq!(polled(&source, &quiet, Some(&cursor)), Ok(None));
    assert_eq!(
        quiet
            .asked()
            .into_iter()
            .map(|(_, etag)| etag)
            .collect::<Vec<_>>(),
        [Some("r1".to_owned()), Some("t1".to_owned())]
    );
    let released = ScriptedHttp::new(vec![
        answer(
            200,
            &json!([release("v4", "2026-11-04T10:00:00Z")]),
            Some("r2"),
        ),
        unchanged(),
        repository(Forge::GitHub),
    ]);
    let found = polled(&source, &released, Some(&cursor)).unwrap().unwrap();
    assert_eq!(found.etag.as_deref(), Some("r2"));
    assert_eq!(found.signals, cursor.signals);
    let tagged = ScriptedHttp::new(vec![
        unchanged(),
        answer(200, &json!([]), Some("t2")),
        repository(Forge::GitHub),
    ]);
    let found = polled(&source, &tagged, Some(&cursor)).unwrap().unwrap();
    assert_eq!(found.etag.as_deref(), Some("r1"));
    assert_eq!(found.last_modified, cursor.last_modified);
    assert_eq!(
        found
            .signals
            .get(&Signal::Tag)
            .and_then(|tag| tag.etag.as_deref()),
        Some("t2")
    );
}

#[test]
fn a_repository_without_a_page_or_an_unreadable_tag_fails_the_poll() {
    let source = releases(Forge::GitHub, &[Signal::Tag], (PrereleaseRule::Either, 0));
    let pageless = ScriptedHttp::new(vec![
        answer(200, &json!([{"name": "v1"}]), None),
        answer(200, &json!({"id": 7, "name": "r"}), None),
    ]);
    assert_eq!(
        polled(&source, &pageless, Some(&Cursor::default())),
        Err(Unpolled::Unreadable(
            "GitHub's repository names no web page of the repository".to_owned()
        ))
    );
    let hostless = ScriptedHttp::new(vec![
        answer(200, &json!([{"name": "v1"}]), None),
        answer(200, &json!({"id": 7, "name": "r", "html_url": "o/r"}), None),
    ]);
    assert_eq!(
        polled(&source, &hostless, Some(&Cursor::default())),
        Err(Unpolled::Unreadable(
            "the repository's page o/r names no host".to_owned()
        ))
    );
    let undated = ScriptedHttp::new(vec![
        answer(200, &json!([{"name": "v1"}]), None),
        repository(Forge::GitHub),
        answer(
            200,
            &json!({"object": {"type": "commit", "sha": "c"}}),
            None,
        ),
        answer(200, &json!({}), None),
    ]);
    assert_eq!(
        polled(&source, &undated, Some(&Cursor::default())),
        Err(Unpolled::Unreadable(
            "GitHub's tags could not be read: the tag v1 has no time".to_owned()
        ))
    );
    let garbled = ScriptedHttp::new(vec![
        answer(200, &json!({"not": "tags"}), None),
        repository(Forge::GitHub),
    ]);
    assert!(matches!(
        polled(&source, &garbled, Some(&Cursor::default())),
        Err(Unpolled::Unreadable(reason)) if reason.contains("GitHub's tags")
    ));
    let unreachable = ScriptedHttp::new(vec![
        answer(200, &json!([{"name": "v1"}]), None),
        repository(Forge::GitHub),
    ]);
    assert!(matches!(
        polled(&source, &unreachable, Some(&Cursor::default())),
        Err(Unpolled::Unreachable(_))
    ));
    let broken = ScriptedHttp::new(vec![
        answer(200, &json!([{"name": "v1"}]), None),
        repository(Forge::GitHub),
        answer(200, &json!({"object": {"type": "tree", "sha": "c"}}), None),
    ]);
    assert!(matches!(
        polled(&source, &broken, Some(&Cursor::default())),
        Err(Unpolled::Unreadable(_))
    ));
}

fn one_tag(forge: Forge, http: &ScriptedHttp) -> Result<Event, Unpolled> {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        api: Some("https://forge.example/api".to_owned()),
        ..SourceOptions::default()
    };
    let origin = Origin::new(forge, &options, "o/r");
    pollster::block_on(tag_event_of(
        &origin,
        http,
        ("v1-rc.1", None),
        PrereleaseRule::Tag,
    ))
}

#[test]
fn one_tag_is_read_in_full_on_each_forge() {
    let gitlab = ScriptedHttp::new(vec![
        repository(Forge::GitLab),
        answer(
            200,
            &json!({"name": "v1-rc.1", "message": "One", "created_at": "2026-11-01T10:00:00Z"}),
            None,
        ),
    ]);
    let event = one_tag(Forge::GitLab, &gitlab).unwrap();
    assert_eq!(
        event.origin().map(|origin| origin.repository.as_str()),
        Some("o/r")
    );
    assert_eq!(
        (event.identity().as_str(), event.body(), event.prerelease()),
        ("forge.example/7/v1-rc.1", "One", true)
    );
    assert_eq!(
        gitlab.asked()[1].0,
        "https://forge.example/api/projects/o%2Fr/repository/tags/v1-rc.1"
    );
    let forgejo = ScriptedHttp::new(vec![
        repository(Forge::Forgejo),
        answer(
            200,
            &json!({"name": "v1-rc.1", "message": "", "id": "c", "commit": {"sha": "c", "created": "2026-11-01T10:00:00Z"}}),
            None,
        ),
    ]);
    assert_eq!(
        one_tag(Forge::Forgejo, &forgejo).map(|event| event.signal()),
        Ok(Some(Signal::Tag))
    );
    assert_eq!(
        forgejo.asked()[1].0,
        "https://forge.example/api/repos/o/r/tags/v1-rc.1"
    );
    let github = ScriptedHttp::new(vec![
        repository(Forge::GitHub),
        answer(
            200,
            &json!({"object": {"type": "commit", "sha": "c"}}),
            None,
        ),
        answer(
            200,
            &json!({"committer": {"date": "2026-11-01T10:00:00Z"}}),
            None,
        ),
    ]);
    assert!(one_tag(Forge::GitHub, &github).is_ok());
    assert_eq!(github.asked().len(), 3);
    let pageless = ScriptedHttp::new(vec![answer(200, &json!({"id": 7, "name": "r"}), None)]);
    assert_eq!(
        one_tag(Forge::GitHub, &pageless),
        Err(Unpolled::Unreadable(
            "GitHub's repository names no web page of the repository".to_owned()
        ))
    );
    let missing = ScriptedHttp::new(vec![
        repository(Forge::GitLab),
        answer(404, &json!({}), None),
    ]);
    assert!(matches!(
        one_tag(Forge::GitLab, &missing),
        Err(Unpolled::Unreadable(_))
    ));
    let unreachable = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        one_tag(Forge::GitLab, &unreachable),
        Err(Unpolled::Unreachable(_))
    ));
    let undescribed = ScriptedHttp::new(vec![answer(500, &json!({}), None)]);
    assert!(matches!(
        one_tag(Forge::GitLab, &undescribed),
        Err(Unpolled::Unreadable(_))
    ));
    let unanswered = ScriptedHttp::new(vec![repository(Forge::GitLab)]);
    assert!(matches!(
        one_tag(Forge::GitLab, &unanswered),
        Err(Unpolled::Unreachable(_))
    ));
}

#[test]
fn a_repository_s_runs_are_polled_with_their_own_validators_and_the_rest_unasked_where_unchanged() {
    let source = releases(
        Forge::GitHub,
        &[Signal::Release, Signal::Run],
        (PrereleaseRule::Either, 0),
    );
    let run = json!({
        "id": 4, "name": "CI", "run_attempt": 2, "status": "completed", "conclusion": "success",
        "html_url": "https://forge.example/o/r/actions/runs/4", "updated_at": "2026-11-02T10:00:00Z"
    });
    let http = ScriptedHttp::new(vec![
        unchanged(),
        answer(200, &json!({"workflow_runs": [run]}), Some("\"runs\"")),
        repository(Forge::GitHub),
    ]);
    let cursor = Cursor {
        etag: Some("\"releases\"".to_owned()),
        ..Cursor::default()
    };
    let found = polled(&source, &http, Some(&cursor)).unwrap().unwrap();
    assert_eq!(
        identities(&found),
        [("forge.example/7/run/4/2", Some(Signal::Run))]
    );
    assert_eq!(found.etag.as_deref(), Some("\"releases\""));
    assert_eq!(
        found.signals[&Signal::Run].etag.as_deref(),
        Some("\"runs\"")
    );
    assert_eq!(
        http.asked()[1].0,
        "https://forge.example/api/repos/o/r/actions/runs?per_page=30&status=completed"
    );
    let quiet = ScriptedHttp::new(vec![unchanged(), unchanged()]);
    assert_eq!(polled(&source, &quiet, Some(&cursor)).unwrap(), None);
    assert_eq!(quiet.asked().len(), 2);
}

#[test]
fn a_repository_s_alerts_are_polled_beside_its_runs_each_under_its_own_validators() {
    let source = Releases {
        alerts: SourceOptions {
            alerts: Some(crate::Alerts {
                tools: Some(vec![crate::Tool::CodeScanning]),
                ..crate::Alerts::default()
            }),
            ..SourceOptions::default()
        }
        .alert_choice(Forge::GitHub),
        ..releases(
            Forge::GitHub,
            &[Signal::Release, Signal::Run, Signal::Alert],
            (PrereleaseRule::Either, 0),
        )
    };
    let alert = json!({
        "number": 3, "state": "open", "html_url": "https://forge.example/o/r/security/code-scanning/3",
        "created_at": "2026-11-02T09:00:00Z"
    });
    let http = ScriptedHttp::new(vec![
        unchanged(),
        unchanged(),
        answer(200, &json!([alert]), Some("\"alerts\"")),
        repository(Forge::GitHub),
    ]);
    let cursor = Cursor {
        etag: Some("\"releases\"".to_owned()),
        ..Cursor::default()
    };
    let found = polled(&source, &http, Some(&cursor)).unwrap().unwrap();
    assert_eq!(
        identities(&found),
        [(
            "forge.example/7/alert/code-scanning/3/created",
            Some(Signal::Alert)
        )]
    );
    assert_eq!(
        found.listings["alert/code-scanning"].etag.as_deref(),
        Some("\"alerts\"")
    );
    assert_eq!(
        http.asked()[2].0,
        "https://forge.example/api/repos/o/r/code-scanning/alerts?sort=updated&direction=desc&per_page=30"
    );
    let quiet = ScriptedHttp::new(vec![unchanged(), unchanged(), unchanged()]);
    assert_eq!(polled(&source, &quiet, Some(&cursor)).unwrap(), None);
    assert_eq!(quiet.asked().len(), 3);
}
