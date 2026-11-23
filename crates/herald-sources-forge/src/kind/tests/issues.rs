use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;

fn opened(path: &str) -> Value {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    json!({
        "action": "opened",
        "issue": {
            "number": 4, "title": "Crash", "state": "open",
            "html_url": format!("https://github.com/{path}/issues/4"),
            "user": { "login": "ann" }, "created_at": "2026-11-18T08:00:00Z",
        },
        "repository": {
            "id": 7, "name": name, "full_name": path, "visibility": "public",
            "html_url": format!("https://github.com/{path}"),
        }
    })
}

const ISSUE: (&str, &str) = ("X-GitHub-Event", "issues");

#[test]
fn an_issue_delivered_to_a_source_announcing_them_is_its_event() {
    let source = signalled(Forge::GitHub, "o/r", true, Signal::Issue);
    let Ok(Hooked::Event(event)) = hooked(&source, ISSUE, &opened("o/r")) else {
        panic!("not an event");
    };
    assert_eq!(event.identity().as_str(), "github.com/7/issue/4/opened");
    let mut synced = opened("o/r");
    synced["action"] = json!("labeled");
    assert_eq!(
        hooked(&source, ISSUE, &synced),
        Ok(Hooked::Nothing(UNCHOSEN_ISSUE))
    );
    let released = ForgeSource {
        options: options("o/r"),
        ..source.clone()
    };
    assert_eq!(
        hooked(&released, ISSUE, &opened("o/r")),
        Ok(Hooked::Nothing(UNISSUED))
    );
    let gitlab = signalled(Forge::GitLab, "group/web", true, Signal::Issue);
    let issue = json!({
        "user": { "username": "ann" },
        "project": { "id": 50, "path_with_namespace": "group/web", "web_url": "https://gitlab.com/group/web" },
        "object_attributes": {
            "iid": 3, "title": "t", "url": "https://gitlab.com/group/web/-/issues/3",
            "action": "open", "created_at": "2026-11-18T08:00:00Z",
        },
    });
    let Ok(Hooked::Event(event)) = hooked(&gitlab, ("X-Gitlab-Event", "Issue Hook"), &issue) else {
        panic!("not an event");
    };
    assert_eq!(event.identity().as_str(), "gitlab.com/50/issue/3/opened");
    assert!(hooked(&gitlab, ("X-Gitlab-Event", "Issue Hook"), &json!({})).is_err());
    assert_eq!(
        hooked(
            &gitlab,
            ("X-Gitlab-Event", "Confidential Issue Hook"),
            &issue
        ),
        Ok(Hooked::Nothing(UNCHOSEN_ISSUE))
    );
}

#[test]
fn doctor_says_what_a_source_announcing_issues_cannot_hear() {
    let noted = |source: &ForgeSource| -> Vec<&'static str> {
        source
            .notes()
            .into_iter()
            .filter(|note| [ISSUES_UNFOLLOWED, ISSUES_POLLED, GITLAB_AUTHORS].contains(note))
            .collect()
    };
    assert_eq!(
        noted(&signalled(Forge::GitHub, "o/r", true, Signal::Issue)),
        Vec::<&str>::new()
    );
    assert_eq!(
        noted(&signalled(Forge::GitHub, "o/r", false, Signal::Issue)),
        [ISSUES_POLLED]
    );
    assert_eq!(
        noted(&signalled(Forge::GitLab, "g/r", true, Signal::Issue)),
        [GITLAB_AUTHORS]
    );
    assert_eq!(
        noted(&signalled(Forge::GitLab, "g/r", false, Signal::Issue)),
        [ISSUES_POLLED]
    );
    let followed = ForgeSource {
        options: SourceOptions {
            repository: None,
            owner: Some("o".to_owned()),
            transport: Some(Transport::Events),
            ..signalled(Forge::GitHub, "o/r", true, Signal::Issue).options
        },
        ..signalled(Forge::GitHub, "o/r", true, Signal::Issue)
    };
    assert_eq!(noted(&followed), [ISSUES_UNFOLLOWED]);
}

#[test]
fn a_gitlab_member_s_hook_sends_what_its_own_signals_take_confidential_issues_only_where_asked() {
    use herald_kinds::Polling;
    use herald_ports::{Response, Secret};

    let member = herald_core::Listed {
        id: "7".to_owned(),
        name: "a".to_owned(),
        path: "group/a".to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: herald_core::Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    };
    let answer = |status: u16, body: &str| Response {
        status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    };
    let quiet = BTreeMap::from([(
        "a".to_owned(),
        crate::Override {
            signals: Some(vec![Signal::Release]),
            ..crate::Override::default()
        },
    )]);
    let cases = [
        (Some(true), BTreeMap::new(), (true, true)),
        (None, BTreeMap::new(), (true, false)),
        (Some(true), quiet, (false, false)),
    ];
    for (confidential, repositories, (taken, sent)) in cases {
        let source = GitLabKind::checked(SourceOptions {
            repository: None,
            owner: Some("group".to_owned()),
            signals: Some(Signal::ALL.to_vec()),
            repositories,
            issues: Some(crate::Issues {
                confidential,
                ..crate::Issues::default()
            }),
            ..options("cli/cli")
        })
        .unwrap();
        let Polling::Many(expand) = source.poller(None, false) else {
            panic!("an owner is polled as its members");
        };
        let scripted =
            herald_testing::ScriptedHttp::new(vec![answer(200, "[]"), answer(201, r#"{"id":1}"#)]);
        let url = "https://herald.example/hooks/group";
        pollster::block_on(expand.register(&scripted, &member, (url, &Secret::new("HOOK", "s"))))
            .unwrap();
        let posted: Value = serde_json::from_slice(&scripted.requests()[1].body).unwrap();
        for key in [
            "pipeline_events",
            "vulnerability_events",
            "merge_requests_events",
            "issues_events",
            "milestone_events",
        ] {
            assert_eq!(posted[key], json!(taken), "{key}");
        }
        assert_eq!(posted["confidential_issues_events"], json!(sent));
    }
}
