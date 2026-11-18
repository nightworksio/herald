//! An owner's members' runs, read beside what GraphQL reads of each, and
//! every member polled each round where runs are announced.

use herald_core::{Identity, Signal};

use super::*;

/// The round each poll here is made in.
const ROUND: jiff::Timestamp = jiff::Timestamp::UNIX_EPOCH;

fn member(id: &str) -> Listed {
    Listed {
        id: id.to_owned(),
        name: "cli".to_owned(),
        path: "lemonfiber/cli".to_owned(),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility: Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    }
}

fn ran(transport: Option<Transport>) -> Owned {
    owned(
        Forge::GitHub,
        &SourceOptions {
            token: Some(herald_core::SecretRef {
                secret: "TOKEN".to_owned(),
            }),
            signals: Some(vec![Signal::Release, Signal::Run]),
            transport,
            ..SourceOptions::default()
        },
    )
}

fn released() -> Response {
    let releases = json!({ "databaseId": 9, "releases": { "nodes": [{
        "tagName": "v1",
        "url": "https://github.com/lemonfiber/cli/releases/tag/v1",
        "isDraft": false,
        "publishedAt": "2026-11-01T12:00:00Z"
    }] } });
    answer(200, &json!({ "data": { "m0": releases } }), "", false)
}

fn runs() -> Response {
    let run = json!({
        "id": 4, "name": "CI", "run_attempt": 1, "status": "completed", "conclusion": "failure",
        "html_url": "https://github.com/lemonfiber/cli/actions/runs/4",
        "updated_at": "2026-11-17T10:00:00Z"
    });
    answer(200, &json!({ "workflow_runs": [run] }), "\"runs\"", false)
}

#[test]
fn a_member_polled_through_graphql_reads_its_runs_beside_and_keeps_its_validators() {
    let source = ran(None);
    let cli = member("9");
    let cursor = Cursor {
        seen: vec![Identity::new("github.com/9/v1")],
        etag: Some("\"releases\"".to_owned()),
        ..Cursor::default()
    };
    let http = ScriptedHttp::new(vec![released(), runs()]);
    let polled = pollster::block_on(source.polled(
        &http,
        &[(&cli, Some(&cursor))],
        &Rounded::Nothing,
        ROUND,
    ));
    let Ok(Some(found)) = &polled[0] else {
        panic!("{polled:?}");
    };
    let identities: Vec<&str> = found
        .events
        .iter()
        .map(|event| event.identity().as_str())
        .collect();
    assert_eq!(identities, ["github.com/9/run/4/1"]);
    assert_eq!(found.etag.as_deref(), Some("\"releases\""));
    assert_eq!(
        found.signals[&Signal::Run].etag.as_deref(),
        Some("\"runs\"")
    );
    assert_eq!(
        http.asked()[1].0,
        "https://api.github.com/repos/lemonfiber/cli/actions/runs?per_page=30&status=completed"
    );
    let quiet = ScriptedHttp::new(vec![released(), unchanged()]);
    let polled = pollster::block_on(source.polled(
        &quiet,
        &[(&cli, Some(&cursor))],
        &Rounded::Nothing,
        ROUND,
    ));
    assert!(matches!(polled[0], Ok(None)), "{polled:?}");
    let unnumbered = member("lemonfiber/cli");
    let http = ScriptedHttp::new(vec![released()]);
    let polled = pollster::block_on(source.polled(
        &http,
        &[(&unnumbered, Some(&cursor))],
        &Rounded::Nothing,
        ROUND,
    ));
    assert!(
        matches!(&polled[0], Err(Unpolled::Unreadable(reason)) if reason.starts_with("the repository lemonfiber/cli's id")),
        "{polled:?}"
    );
}

#[test]
fn an_owner_announcing_runs_polls_every_member_whatever_its_events_say() {
    let source = ran(Some(Transport::Events));
    let nothing = ScriptedHttp::new(Vec::new());
    assert!(pollster::block_on(source.changed(&nothing, Owner::Organization, None)).is_none());
    assert_eq!(nothing.asked(), Vec::new());
}
