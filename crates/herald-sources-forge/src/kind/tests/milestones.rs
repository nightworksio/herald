use serde_json::{Value, json};

use super::*;

fn closed(path: &str, visibility: &str) -> Value {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    json!({
        "action": "closed",
        "milestone": {
            "id": 1003, "number": 3, "title": "v1.4", "state": "closed",
            "created_at": "2026-11-18T08:00:00Z", "closed_at": "2026-11-18T11:00:00Z",
            "html_url": format!("https://github.com/{path}/milestone/3"),
        },
        "repository": {
            "id": 7, "name": name, "full_name": path, "visibility": visibility,
            "html_url": format!("https://github.com/{path}"),
        }
    })
}

const MILESTONE: (&str, &str) = ("X-GitHub-Event", "milestone");

#[test]
fn a_milestone_delivered_to_a_source_announcing_them_is_its_event_of_its_repository() {
    let source = signalled(Forge::GitHub, "o/r", true, Signal::Milestone);
    let Ok(Hooked::Event(event)) = hooked(&source, MILESTONE, &closed("o/r", "private")) else {
        panic!("not an event");
    };
    assert_eq!(
        event.identity().as_str(),
        "github.com/7/milestone/3/closed/1794999600"
    );
    assert_eq!(event.bounded_by(), Some(herald_core::Visibility::Private));
    let mut created = closed("o/r", "public");
    created["action"] = json!("created");
    assert_eq!(
        hooked(&source, MILESTONE, &created),
        Ok(Hooked::Nothing(UNCHOSEN_MILESTONE))
    );
    let released = ForgeSource {
        options: options("o/r"),
        ..source.clone()
    };
    assert_eq!(
        hooked(&released, MILESTONE, &closed("o/r", "public")),
        Ok(Hooked::Nothing(UNMILESTONED))
    );
    assert!(hooked(&source, MILESTONE, &closed("o/other", "public")).is_err());
    let gitlab = signalled(Forge::GitLab, "group/web", true, Signal::Milestone);
    let attributes = json!({
        "id": 61, "iid": 2, "title": "v1.4", "state": "closed", "project_id": 50,
        "created_at": "2026-11-18 08:00:00 UTC", "updated_at": "2026-11-18 10:00:00 UTC",
    });
    let milestone = json!({
        "action": "close", "object_attributes": attributes,
        "project": { "id": 50, "path_with_namespace": "group/web", "web_url": "https://gitlab.com/group/web", "visibility_level": 20 },
    });
    let hook = ("X-Gitlab-Event", "Milestone Hook");
    let Ok(Hooked::Event(event)) = hooked(&gitlab, hook, &milestone) else {
        panic!("not an event");
    };
    assert_eq!(
        event.identity().as_str(),
        "gitlab.com/50/milestone/61/closed/1794996000"
    );
    assert_eq!(event.bounded_by(), Some(herald_core::Visibility::Public));
    let grouped = json!({ "action": "close", "object_attributes": attributes });
    assert_eq!(
        hooked(&gitlab, hook, &grouped),
        Ok(Hooked::Nothing(UNPROJECTED_MILESTONE))
    );
    assert!(hooked(&gitlab, hook, &json!({})).is_err());
}

#[test]
fn doctor_says_what_a_poll_of_milestones_costs_and_what_each_forge_cannot_hear() {
    let noted = |source: &ForgeSource| -> Vec<&'static str> {
        source
            .notes()
            .into_iter()
            .filter(|note| {
                [
                    MILESTONES_COST,
                    MILESTONES_UNFOLLOWED,
                    MILESTONES_POLLED,
                    FORGEJO_MILESTONES,
                    GITLAB_MILESTONE_HOOK,
                    GITLAB_COUNTS,
                    GITHUB_COUNTS,
                ]
                .contains(note)
            })
            .collect()
    };
    assert_eq!(
        noted(&signalled(Forge::GitHub, "o/r", true, Signal::Milestone)),
        [MILESTONES_COST, GITHUB_COUNTS]
    );
    assert_eq!(
        noted(&signalled(Forge::GitHub, "o/r", false, Signal::Milestone)),
        [MILESTONES_COST, MILESTONES_POLLED, GITHUB_COUNTS]
    );
    assert_eq!(
        noted(&signalled(Forge::GitLab, "g/r", true, Signal::Milestone)),
        [MILESTONES_COST, GITLAB_MILESTONE_HOOK, GITLAB_COUNTS]
    );
    assert_eq!(
        noted(&signalled(Forge::Forgejo, "o/r", false, Signal::Milestone)),
        [MILESTONES_COST, MILESTONES_POLLED, FORGEJO_MILESTONES]
    );
    assert_eq!(
        noted(&signalled(Forge::GitHub, "o/r", true, Signal::Issue)),
        Vec::<&str>::new()
    );
    let uncounted = ForgeSource {
        options: SourceOptions {
            milestones: Some(crate::Milestones {
                counts: Some(false),
                ..crate::Milestones::default()
            }),
            ..signalled(Forge::GitLab, "g/r", true, Signal::Milestone).options
        },
        ..signalled(Forge::GitLab, "g/r", true, Signal::Milestone)
    };
    assert_eq!(noted(&uncounted), [MILESTONES_COST, GITLAB_MILESTONE_HOOK]);
    let followed = ForgeSource {
        options: SourceOptions {
            repository: None,
            owner: Some("o".to_owned()),
            transport: Some(Transport::Events),
            ..signalled(Forge::GitHub, "o/r", true, Signal::Milestone).options
        },
        ..signalled(Forge::GitHub, "o/r", true, Signal::Milestone)
    };
    assert_eq!(
        noted(&followed),
        [MILESTONES_COST, MILESTONES_UNFOLLOWED, GITHUB_COUNTS]
    );
}

#[test]
fn a_poll_s_milestones_a_group_s_among_them_are_bound_to_their_project_s_visibility() {
    use herald_kinds::Polling;
    use herald_ports::Response;

    let answer = |body: Value| Response {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    };
    let milestone = |id: u64, state: &str, group: bool| {
        json!({
            "id": id, "iid": id, "title": "v1.4", "state": state,
            "project_id": (!group).then_some(50), "group_id": group.then_some(9),
            "created_at": "2026-11-18T08:00:00Z", "updated_at": "2026-11-18T10:00:00Z",
        })
    };
    let counts = json!({ "statistics": { "counts": { "all": 2, "closed": 1, "opened": 1 } } });
    let cursor = herald_core::Cursor {
        ids: std::collections::BTreeMap::from([
            ("milestone".to_owned(), vec!["1".to_owned()]),
            ("milestone-group".to_owned(), Vec::new()),
        ]),
        ..herald_core::Cursor::default()
    };
    for (visibility, bound) in [
        ("private", herald_core::Visibility::Private),
        ("internal", herald_core::Visibility::Internal),
        ("public", herald_core::Visibility::Public),
    ] {
        let source = ForgeSource {
            options: SourceOptions {
                milestones: Some(crate::Milestones {
                    actions: Some(vec![
                        crate::MilestoneAction::Opened,
                        crate::MilestoneAction::Closed,
                    ]),
                    group: Some(true),
                    ..crate::Milestones::default()
                }),
                ..signalled(Forge::GitLab, "group/web", false, Signal::Milestone).options
            },
            ..signalled(Forge::GitLab, "group/web", false, Signal::Milestone)
        };
        let Polling::One(poll) = source.poller(None, false) else {
            panic!("a repository is polled alone");
        };
        let http = herald_testing::ScriptedHttp::new(vec![
            answer(json!([])),
            answer(milestone(1, "closed", false)),
            answer(counts.clone()),
            answer(json!([milestone(2, "active", true)])),
            answer(counts.clone()),
            answer(json!({
                "id": 50, "name": "web", "path_with_namespace": "group/web",
                "web_url": "https://gitlab.com/group/web", "visibility": visibility,
            })),
        ]);
        let polled =
            pollster::block_on(poll.polled(&http, Some(&cursor), jiff::Timestamp::UNIX_EPOCH))
                .unwrap()
                .unwrap();
        let read: Vec<(&str, Option<herald_core::Visibility>)> = polled
            .events
            .iter()
            .map(|event| (event.identity().as_str(), event.bounded_by()))
            .collect();
        assert_eq!(
            read,
            [
                ("gitlab.com/50/milestone/1/closed/1794996000", Some(bound)),
                ("gitlab.com/group/9/milestone/2/opened", Some(bound)),
            ],
            "{visibility}"
        );
        assert_eq!(polled.ids["milestone/closed"], ["1"]);
    }
}
