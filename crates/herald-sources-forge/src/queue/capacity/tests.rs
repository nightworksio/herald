use herald_core::{Capacity, Listed, Overviews, Owner, Visibility};
use herald_kinds::{Asked, Queue as _, Scope};
use herald_ports::{Response, Secret};
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::queue::Queued;
use crate::{Origin, SourceOptions, Token};

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn queued(forge: Forge) -> Queued {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    Queued {
        origin: Origin::new(forge, &options, "o/r"),
        token: Some(Token::Named(Secret::new("TOKEN", "t"))),
        project: "r".to_owned(),
        graphql: "https://api.github.com/graphql".to_owned(),
    }
}

fn counted(
    queue: &dyn herald_kinds::Queue,
    answers: Vec<Response>,
    (asked, shown, capacity): (Asked<'_>, &[&str], Capacity),
) -> (Overviews, Vec<String>) {
    let http = ScriptedHttp::new(answers);
    let overviews = pollster::block_on(queue.capacity(&http, (asked, shown), capacity)).unwrap();
    let asked = http.asked().into_iter().map(|(url, _)| url).collect();
    (overviews, asked)
}

fn ids(overviews: &Overviews) -> Vec<(String, bool, bool)> {
    overviews
        .capacity
        .iter()
        .flatten()
        .map(|run| (run.id.clone(), run.queued, run.pull))
        .collect()
}

const ONE: (Asked<'static>, &[&str], Capacity) = (
    Asked {
        most: herald_kinds::Most {
            open: 100,
            pulls: 20,
        },
        scope: None,
        stated: false,
        files: false,
    },
    &["o/r"],
    Capacity::Shown,
);

#[test]
fn github_counts_its_queued_and_running_workflow_runs() {
    let run = |id: u64, event: &str| {
        json!({ "id": id, "html_url": format!("https://github.com/o/r/actions/runs/{id}"),
            "head_sha": "h1", "event": event, "created_at": "2026-11-20T10:00:00Z" })
    };
    let (overviews, asked) = counted(
        &queued(Forge::GitHub),
        vec![
            answer(200, &json!({ "workflow_runs": [run(1, "pull_request")] })),
            answer(200, &json!({ "workflow_runs": [run(2, "push")] })),
        ],
        ONE,
    );
    assert_eq!(
        ids(&overviews),
        [("1".to_owned(), true, true), ("2".to_owned(), false, false)]
    );
    assert_eq!(
        asked,
        [
            "https://api.github.com/repos/o/r/actions/runs?status=queued&per_page=100",
            "https://api.github.com/repos/o/r/actions/runs?status=in_progress&per_page=100",
        ]
    );
    assert_eq!(
        overviews.capacity.unwrap()[0].visibility,
        Visibility::Private
    );
}

#[test]
fn gitlab_counts_pending_and_running_pipelines_and_forgejo_its_waiting_and_running_tasks() {
    let pipeline = |id: u64, source: &str| {
        json!({ "id": id, "web_url": format!("https://gitlab.com/o/r/-/pipelines/{id}"),
            "sha": "h1", "source": source, "created_at": "2026-11-20T10:00:00Z" })
    };
    let (overviews, asked) = counted(
        &queued(Forge::GitLab),
        vec![
            answer(200, &json!([pipeline(7, "merge_request_event")])),
            answer(200, &json!([pipeline(8, "push")])),
        ],
        ONE,
    );
    assert_eq!(
        ids(&overviews),
        [("7".to_owned(), true, true), ("8".to_owned(), false, false)]
    );
    assert_eq!(
        asked[0],
        "https://gitlab.com/api/v4/projects/o%2Fr/pipelines?status=pending&per_page=100"
    );
    let task = |id: u64, status: &str| {
        json!({ "id": id, "url": format!("https://codeberg.org/o/r/actions/runs/{id}"),
            "head_sha": "h1", "event": "pull_request", "status": status,
            "created_at": "2026-11-20T10:00:00Z" })
    };
    let (overviews, asked) = counted(
        &queued(Forge::Forgejo),
        vec![answer(
            200,
            &json!({ "workflow_runs": [task(1, "waiting"), task(2, "running"), task(3, "success")] }),
        )],
        ONE,
    );
    assert_eq!(
        ids(&overviews),
        [("1".to_owned(), true, true), ("2".to_owned(), false, true)]
    );
    assert_eq!(
        asked,
        ["https://codeberg.org/api/v1/repos/o/r/actions/tasks?limit=50"]
    );
}

#[test]
fn a_listing_that_fills_its_page_is_named_as_holding_more_than_it_counts() {
    let task = |id: usize| {
        json!({ "id": id, "url": format!("https://codeberg.org/o/r/actions/runs/{id}"),
            "head_sha": "h1", "event": "push", "status": "running",
            "created_at": "2026-11-20T10:00:00Z" })
    };
    let page = |count: usize| {
        answer(
            200,
            &json!({ "workflow_runs": (1..=count).map(task).collect::<Vec<_>>() }),
        )
    };
    let (full, _) = counted(&queued(Forge::Forgejo), vec![page(FORGEJO_PAGE)], ONE);
    let reasons: Vec<String> = full
        .unread
        .iter()
        .map(|unread| unread.reason.clone())
        .collect();
    assert_eq!(
        reasons,
        [format!("{MORE} 50 of its runs; only those 50 are counted")]
    );
    assert_eq!(full.capacity.map(|runs| runs.len()), Some(FORGEJO_PAGE));
    let (short, _) = counted(&queued(Forge::Forgejo), vec![page(FORGEJO_PAGE - 1)], ONE);
    assert_eq!(short.unread, []);
}

#[test]
fn a_repository_whose_runs_are_refused_is_named_unread_with_the_permission_it_needs() {
    for (forge, status, reason) in [
        (Forge::GitHub, 403, GITHUB_REFUSED),
        (Forge::GitHub, 404, UNSEEN),
        (Forge::GitLab, 401, GITLAB_REFUSED),
        (Forge::Forgejo, 403, FORGEJO_REFUSED),
        (Forge::Forgejo, 404, FORGEJO_UNLISTED),
    ] {
        let (overviews, _) = counted(&queued(forge), vec![answer(status, &json!({}))], ONE);
        assert_eq!(overviews.capacity, Some(Vec::new()), "{forge:?}");
        assert_eq!(overviews.unread.len(), 1, "{forge:?}");
        assert_eq!(overviews.unread[0].reason, reason, "{forge:?}");
        assert_eq!(overviews.unread[0].repository, "o/r");
    }
    for (forge, what) in [
        (Forge::GitHub, RUNS),
        (Forge::GitLab, PIPELINES),
        (Forge::Forgejo, TASKS),
    ] {
        let http = ScriptedHttp::new(vec![answer(500, &json!({}))]);
        let failed =
            pollster::block_on(queued(forge).capacity(&http, (ONE.0, &["o/r"]), Capacity::Shown));
        let Err(Unpolled::Unreadable(reason)) = failed else {
            panic!("{forge:?}: {failed:?}");
        };
        assert!(reason.contains(what), "{forge:?}: {reason}");
    }
    let http = ScriptedHttp::new(vec![answer(200, &json!("runs"))]);
    let failed = pollster::block_on(queued(Forge::GitLab).capacity(
        &http,
        (ONE.0, &["o/r"]),
        Capacity::Shown,
    ));
    assert!(matches!(failed, Err(Unpolled::Unreadable(_))));
}

fn member(path: &str, visibility: Visibility) -> Listed {
    Listed {
        id: path.to_owned(),
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        path: path.to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    }
}

fn scoped(forge: Forge) -> crate::queue::Scoped {
    let options = SourceOptions {
        owner: Some("o".to_owned()),
        ..SourceOptions::default()
    };
    let owned = crate::owner::Owned::new(forge, ("o", None), &options, (None, 0));
    crate::queue::Scoped::new(
        (forge, "o"),
        &options,
        None,
        ("https://api.github.com/graphql".to_owned(), owned),
    )
}

#[test]
fn an_owner_counts_the_repositories_it_shows_or_its_newest_pushed_members() {
    let members = [
        member("o/app", Visibility::Public),
        member("o/vault", Visibility::Private),
        member("o/quiet", Visibility::Public),
    ];
    let scope = Scope {
        owner: Owner::Organization,
        members: &members,
    };
    let asked = Asked {
        scope: Some(scope),
        ..ONE.0
    };
    let empty = || answer(200, &json!({ "workflow_runs": [] }));
    let (overviews, asked_urls) = counted(
        &scoped(Forge::GitHub),
        vec![empty(), empty()],
        (asked, &["o/app", "o/elsewhere"], Capacity::Shown),
    );
    assert_eq!(overviews.capacity, Some(Vec::new()));
    assert_eq!(asked_urls.len(), 2);
    assert!(asked_urls[0].contains("/repos/o/app/actions/runs?status=queued"));
    let pushed =
        json!([{ "full_name": "o/vault" }, { "full_name": "o/gone" }, { "full_name": "o/app" }]);
    let (_, asked_urls) = counted(
        &scoped(Forge::GitHub),
        vec![answer(200, &pushed), empty(), empty(), empty(), empty()],
        (asked, &[], Capacity::Members { most: 2 }),
    );
    assert_eq!(
        asked_urls[0],
        "https://api.github.com/orgs/o/repos?sort=pushed&direction=desc&per_page=2"
    );
    assert!(asked_urls[1].contains("/repos/o/vault/"), "{asked_urls:?}");
    assert!(asked_urls[3].contains("/repos/o/app/"), "{asked_urls:?}");
    let refused = ScriptedHttp::new(vec![answer(403, &json!({}))]);
    let unread = pollster::block_on(scoped(Forge::GitHub).capacity(
        &refused,
        (asked, &["o/vault"]),
        Capacity::Shown,
    ))
    .unwrap();
    assert_eq!(unread.unread[0].visibility, Visibility::Private);
    let (_, forgejo) = counted(
        &scoped(Forge::Forgejo),
        vec![answer(200, &json!({ "workflow_runs": [] }))],
        (asked, &[], Capacity::Members { most: 1 }),
    );
    assert_eq!(forgejo.len(), 1);
    assert!(
        forgejo[0].contains("/repos/o/app/actions/tasks"),
        "{forgejo:?}"
    );
    let (none, _) = counted(
        &scoped(Forge::GitHub),
        Vec::new(),
        (ONE.0, &[], Capacity::Shown),
    );
    assert_eq!(none.capacity, Some(Vec::new()));
}

#[test]
fn a_user_or_group_s_newest_pushed_repositories_are_asked_where_their_forge_lists_them() {
    let members = [member("o/app", Visibility::Public)];
    for (forge, owner, wanted) in [
        (
            Forge::GitHub,
            Owner::User,
            "https://api.github.com/users/o/repos?sort=pushed&direction=desc&per_page=1",
        ),
        (
            Forge::GitLab,
            Owner::Organization,
            "https://gitlab.com/api/v4/groups/o/projects?order_by=last_activity_at&sort=desc&include_subgroups=true&per_page=1",
        ),
        (
            Forge::GitLab,
            Owner::User,
            "https://gitlab.com/api/v4/users/o/projects?order_by=last_activity_at&sort=desc&per_page=1",
        ),
    ] {
        let asked = Asked {
            scope: Some(Scope {
                owner,
                members: &members,
            }),
            ..ONE.0
        };
        let http = ScriptedHttp::new(vec![answer(200, &json!([]))]);
        let overviews = pollster::block_on(scoped(forge).capacity(
            &http,
            (asked, &[]),
            Capacity::Members { most: 1 },
        ))
        .unwrap();
        assert_eq!(overviews.capacity, Some(Vec::new()));
        assert_eq!(http.asked()[0].0, wanted, "{forge:?}");
    }
    let asked = Asked {
        scope: Some(Scope {
            owner: Owner::Organization,
            members: &members,
        }),
        ..ONE.0
    };
    let http = ScriptedHttp::new(vec![answer(200, &json!({}))]);
    let failed = pollster::block_on(scoped(Forge::GitHub).capacity(
        &http,
        (asked, &[]),
        Capacity::Members { most: 1 },
    ));
    assert!(matches!(failed, Err(Unpolled::Unreadable(_))));
}

#[test]
fn a_repository_the_view_does_not_show_is_not_read() {
    let (overviews, asked) = counted(
        &queued(Forge::GitHub),
        Vec::new(),
        (ONE.0, &["o/other"], Capacity::Shown),
    );
    assert_eq!(overviews.capacity, Some(Vec::new()));
    assert_eq!(asked.len(), 0);
}
