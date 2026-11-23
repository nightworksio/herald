//! An owner's members' milestones, followed beside what GraphQL reads of
//! each, and every member polled each round where milestones are announced.

use herald_core::{Identity, Signal};

use super::*;

fn milestone(state: &str) -> Value {
    json!({
        "id": 1003, "number": 3, "title": "v1.4", "state": state,
        "created_at": "2026-11-18T08:00:00Z", "closed_at": "2026-11-18T11:00:00Z",
        "html_url": "https://github.com/lemonfiber/cli/milestone/3",
    })
}

#[test]
fn a_member_polled_through_graphql_follows_its_milestones_beside_and_keeps_their_ids() {
    let source = signalling(Signal::Milestone, None);
    let cli = member();
    let cursor = Cursor {
        seen: vec![Identity::new("github.com/9/v0")],
        ids: BTreeMap::from([("milestone".to_owned(), vec!["3".to_owned()])]),
        ..Cursor::default()
    };
    let http = ScriptedHttp::new(vec![
        released(),
        answer(200, &json!([]), "\"milestones\"", false),
        answer(200, &milestone("closed"), "", false),
    ]);
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
    assert_eq!(identities, ["github.com/9/milestone/3/closed/1794999600"]);
    assert_eq!(
        found.listings["milestone"].etag.as_deref(),
        Some("\"milestones\"")
    );
    assert_eq!(found.ids["milestone"], Vec::<String>::new());
    assert_eq!(found.ids["milestone/closed"], ["3"]);
    assert_eq!(
        http.asked()[1].0,
        "https://api.github.com/repos/lemonfiber/cli/milestones?state=open&per_page=30&page=1"
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
fn an_owner_announcing_milestones_polls_every_member_whatever_its_events_say() {
    let source = signalling(Signal::Milestone, Some(Transport::Events));
    let nothing = ScriptedHttp::new(Vec::new());
    assert!(pollster::block_on(source.changed(&nothing, Owner::Organization, None)).is_none());
    assert_eq!(nothing.asked(), Vec::new());
}
