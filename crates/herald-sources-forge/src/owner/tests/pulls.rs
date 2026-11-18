//! An owner's members' pull requests, read beside what GraphQL reads of each, and
//! every member polled each round where pull requests are announced.

use herald_core::{Identity, Signal};

use super::*;

fn pulls() -> Response {
    let pull = json!({
        "number": 2, "title": "Bump", "state": "open",
        "html_url": "https://github.com/lemonfiber/cli/pull/2",
        "user": { "login": "dependabot[bot]", "type": "Bot" },
        "created_at": "2026-11-18T08:00:00Z",
    });
    answer(200, &json!([pull]), "\"pulls\"", false)
}

#[test]
fn a_member_polled_through_graphql_reads_its_pull_requests_beside_and_keeps_their_validators() {
    let source = signalling(Signal::PullRequest, None);
    let cli = member();
    let cursor = Cursor {
        seen: vec![Identity::new("github.com/9/v0")],
        ..Cursor::default()
    };
    let http = ScriptedHttp::new(vec![released(), pulls()]);
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
    assert_eq!(identities, ["github.com/9/pull/2/opened"]);
    assert_eq!(
        found.listings["pull-request"].etag.as_deref(),
        Some("\"pulls\"")
    );
    assert_eq!(
        http.asked()[1].0,
        "https://api.github.com/repos/lemonfiber/cli/pulls?state=all&sort=updated&direction=desc&per_page=30"
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
fn an_owner_announcing_pull_requests_polls_every_member_whatever_its_events_say() {
    let source = signalling(Signal::PullRequest, Some(Transport::Events));
    let nothing = ScriptedHttp::new(Vec::new());
    assert!(pollster::block_on(source.changed(&nothing, Owner::Organization, None)).is_none());
    assert_eq!(nothing.asked(), Vec::new());
}
