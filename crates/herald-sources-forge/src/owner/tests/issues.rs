//! An owner's members' issues, read beside what GraphQL reads of each, and
//! every member polled each round where issues are announced.

use herald_core::{Identity, Signal};

use super::*;

fn issues() -> Response {
    let issue = json!({
        "number": 2, "title": "Crash", "state": "open",
        "html_url": "https://github.com/lemonfiber/cli/issues/2",
        "user": { "login": "dependabot[bot]", "type": "Bot" },
        "created_at": "2026-11-18T08:00:00Z",
    });
    answer(200, &json!([issue]), "\"issues\"", false)
}

#[test]
fn a_member_polled_through_graphql_reads_its_issues_beside_and_keeps_their_validators() {
    let source = signalling(Signal::Issue, None);
    let cli = member();
    let cursor = Cursor {
        seen: vec![Identity::new("github.com/9/v0")],
        ..Cursor::default()
    };
    let http = ScriptedHttp::new(vec![released(), issues()]);
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
    assert_eq!(identities, ["github.com/9/issue/2/opened"]);
    assert_eq!(found.listings["issue"].etag.as_deref(), Some("\"issues\""));
    assert_eq!(
        http.asked()[1].0,
        "https://api.github.com/repos/lemonfiber/cli/issues?state=all&sort=updated&direction=desc&per_page=30"
    );
    let quiet = ScriptedHttp::new(vec![released(), unchanged()]);
    let polled = pollster::block_on(source.polled(
        &quiet,
        &[(&cli, Some(&cursor))],
        &Rounded::Nothing,
        ROUND,
    ));
    assert!(matches!(polled[0], Ok(None)), "{polled:?}");
}

#[test]
fn an_owner_announcing_issues_polls_every_member_whatever_its_events_say() {
    let source = signalling(Signal::Issue, Some(Transport::Events));
    let nothing = ScriptedHttp::new(Vec::new());
    assert!(pollster::block_on(source.changed(&nothing, Owner::Organization, None)).is_none());
    assert_eq!(nothing.asked(), Vec::new());
}
