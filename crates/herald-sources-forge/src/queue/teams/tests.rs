use herald_kinds::Queue as _;
use herald_ports::{Response, Secret};
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::SourceOptions;
use crate::queue::Queued;

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn logins(names: &[&str]) -> Value {
    Value::Array(
        names
            .iter()
            .map(|login| json!({ "login": login }))
            .collect(),
    )
}

fn origin(forge: Forge, api: Option<&str>, repository: &str) -> Origin {
    let options = SourceOptions {
        repository: Some(repository.to_owned()),
        api: api.map(ToOwned::to_owned),
        ..SourceOptions::default()
    };
    Origin::new(forge, &options, repository)
}

fn read(http: &ScriptedHttp, origin: &Origin, owner: &str) -> Result<Teamed, Unpolled> {
    let token = Token::Named(Secret::new("TOKEN", "t"));
    pollster::block_on(teams(&Erased(http), (origin, Some(&token)), owner))
}

fn urls(http: &ScriptedHttp) -> Vec<String> {
    http.asked().into_iter().map(|(url, _)| url).collect()
}

fn team(name: &str, members: &[&str]) -> (String, Team) {
    let members: Vec<String> = members.iter().map(|member| (*member).to_owned()).collect();
    (name.to_owned(), Team::of(name, &members).unwrap())
}

#[test]
fn github_s_teams_are_each_org_team_with_their_members_on_the_api_s_own_host() {
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([{ "slug": "web" }, { "slug": "api" }])),
        answer(200, &logins(&["Ann", "ben"])),
        answer(200, &logins(&["cat"])),
    ]);
    let origin = origin(Forge::GitHub, Some("https://ghe.example/api/v3/"), "o/r");
    let read = read(&http, &origin, "o");
    let expected = [
        team("o/web", &["ann@ghe.example", "ben@ghe.example"]),
        team("o/api", &["cat@ghe.example"]),
    ];
    assert_eq!(read, Ok(Teamed::Read(expected.into_iter().collect())));
    assert_eq!(
        urls(&http),
        [
            "https://ghe.example/api/v3/orgs/o/teams?per_page=100&page=1",
            "https://ghe.example/api/v3/orgs/o/teams/web/members?per_page=100&page=1",
            "https://ghe.example/api/v3/orgs/o/teams/api/members?per_page=100&page=1",
        ]
    );
    assert!(http.requests().iter().all(|request| {
        request
            .headers
            .iter()
            .any(|(name, _)| name == "Authorization")
    }));
}

#[test]
fn gitlab_s_teams_are_the_group_and_each_group_under_it_by_full_path() {
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([{ "id": 5, "full_path": "g/sub/web" }])),
        answer(200, &json!([{ "username": "ann" }])),
        answer(200, &json!([{ "username": "ben" }])),
    ]);
    let origin = origin(
        Forge::GitLab,
        Some("https://gitlab.example:8443/api/v4"),
        "g/sub/p",
    );
    let owner = owner_of(Forge::GitLab, origin.path());
    assert_eq!(owner, "g/sub");
    let expected = [
        team("g/sub", &["ann@gitlab.example"]),
        team("g/sub/web", &["ben@gitlab.example"]),
    ];
    assert_eq!(
        read(&http, &origin, owner),
        Ok(Teamed::Read(expected.into_iter().collect()))
    );
    assert_eq!(
        urls(&http),
        [
            "https://gitlab.example:8443/api/v4/groups/g%2Fsub/descendant_groups?per_page=100&page=1",
            "https://gitlab.example:8443/api/v4/groups/g%2Fsub/members?per_page=100&page=1",
            "https://gitlab.example:8443/api/v4/groups/5/members?per_page=100&page=1",
        ]
    );
}

#[test]
fn forgejo_s_teams_are_each_org_team_read_until_an_empty_page() {
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([{ "id": 3, "name": "Owners" }])),
        answer(200, &json!([])),
        answer(200, &logins(&["ann"])),
        answer(200, &json!([])),
    ]);
    let origin = origin(Forge::Forgejo, Some("https://forge.example/api/v1"), "o/r");
    assert_eq!(
        read(&http, &origin, "o"),
        Ok(Teamed::Read(
            [team("o/Owners", &["ann@forge.example"])]
                .into_iter()
                .collect()
        ))
    );
    assert_eq!(
        urls(&http),
        [
            "https://forge.example/api/v1/orgs/o/teams?limit=50&page=1",
            "https://forge.example/api/v1/orgs/o/teams?limit=50&page=2",
            "https://forge.example/api/v1/teams/3/members?limit=50&page=1",
            "https://forge.example/api/v1/teams/3/members?limit=50&page=2",
        ]
    );
}

#[test]
fn a_forge_refusing_teams_or_members_is_refused_with_the_permission_never_guessed() {
    for status in [401, 403, 404] {
        let http = ScriptedHttp::new(vec![answer(status, &json!({}))]);
        let origin = origin(Forge::GitHub, None, "o/r");
        assert_eq!(
            read(&http, &origin, "o"),
            Ok(Teamed::Refused(permission(Forge::GitHub)))
        );
    }
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([{ "slug": "web" }])),
        answer(403, &json!({})),
    ]);
    let origin = origin(Forge::GitHub, None, "o/r");
    assert_eq!(
        read(&http, &origin, "o"),
        Ok(Teamed::Refused(permission(Forge::GitHub)))
    );
    for forge in [Forge::GitHub, Forge::GitLab, Forge::Forgejo] {
        assert!(note(forge).contains(permission(forge)), "{forge:?}");
    }
    assert!(permission(Forge::GitHub).contains("read:org"));
    assert!(permission(Forge::GitHub).contains("GHE.com"));
    assert!(permission(Forge::GitLab).contains("read_api"));
    assert!(permission(Forge::GitLab).contains("self-managed"));
    assert!(permission(Forge::Forgejo).contains("read:organization"));
}

#[test]
fn a_forge_s_rate_limit_stops_the_read_on_each_forge_never_read_as_a_refusal() {
    let spent = Response {
        status: 403,
        headers: vec![("x-ratelimit-remaining".to_owned(), "0".to_owned())],
        body: b"{}".to_vec(),
    };
    let secondary = answer(
        403,
        &json!({ "message": "You have exceeded a secondary rate limit." }),
    );
    let teams = || answer(200, &json!([{ "slug": "web" }]));
    let cases = [
        (Forge::GitHub, vec![spent], "team listing"),
        (
            Forge::GitHub,
            vec![teams(), secondary],
            "team member listing",
        ),
        (Forge::GitLab, vec![answer(429, &json!({}))], "team listing"),
        (
            Forge::Forgejo,
            vec![answer(429, &json!({}))],
            "team listing",
        ),
    ];
    for (forge, answers, what) in cases {
        let http = ScriptedHttp::new(answers);
        let origin = origin(forge, Some("https://forge.example/api"), "o/r");
        assert_eq!(
            read(&http, &origin, "o"),
            Err(Unpolled::RateLimited(format!(
                "{what} could not be read: the forge's rate limit was reached; it did not say when the limit resets"
            ))),
            "{forge:?}"
        );
    }
}

#[test]
fn a_forge_unreached_or_unread_stops_the_read_and_names_no_member() {
    let origin = origin(Forge::GitHub, None, "o/r");
    let http = ScriptedHttp::new(vec![answer(200, &json!([{ "slug": "secret-team" }]))]);
    let Err(Unpolled::Unreachable(why)) = read(&http, &origin, "o") else {
        panic!("a forge unreached stops the read");
    };
    assert_eq!(why, "the forge's team member listing could not be reached");
    let http = ScriptedHttp::new(vec![answer(500, &json!({}))]);
    assert!(matches!(
        read(&http, &origin, "o"),
        Err(Unpolled::Unreadable(_))
    ));
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([{ "slug": "web" }])),
        answer(200, &json!([{ "name": "secret-login" }])),
    ]);
    let Err(Unpolled::Unreadable(why)) = read(&http, &origin, "o") else {
        panic!("a member listing herald cannot read is unread");
    };
    assert!(!why.contains("secret-login"), "{why}");
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([{ "slug": "web" }, { "slug": "ops" }])),
        answer(200, &logins(&["has space"])),
        answer(200, &logins(&["ann"])),
    ]);
    assert_eq!(
        read(&http, &origin, "o"),
        Ok(Teamed::Read(
            [team("o/ops", &["ann@github.com"])].into_iter().collect()
        ))
    );
    let http = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        read(&http, &origin, ".."),
        Err(Unpolled::Unreadable(_))
    ));
    assert_eq!(urls(&http), Vec::<String>::new());
}

#[test]
fn a_read_takes_at_most_its_bound_of_teams_and_of_members_a_team() {
    let full: Vec<Value> = (0..100)
        .map(|n| json!({ "slug": format!("t{n}") }))
        .collect();
    let mut answers = vec![answer(200, &Value::Array(full))];
    answers.extend((0..100).map(|_| answer(200, &logins(&["ann"]))));
    let http = ScriptedHttp::new(answers);
    let github = origin(Forge::GitHub, None, "o/r");
    let Ok(Teamed::Read(found)) = read(&http, &github, "o") else {
        panic!("a full page of teams is read");
    };
    assert_eq!(found.len(), MOST_FORGE_TEAMS);
    assert_eq!(urls(&http).len(), 1 + MOST_FORGE_TEAMS);
    let mut answers = vec![answer(200, &json!([{ "slug": "big" }]))];
    for page in 0..6 {
        let members: Vec<String> = (0..100).map(|n| format!("m{page}-{n}")).collect();
        let members: Vec<&str> = members.iter().map(String::as_str).collect();
        answers.push(answer(200, &logins(&members)));
    }
    let http = ScriptedHttp::new(answers);
    let Ok(Teamed::Read(found)) = read(&http, &github, "o") else {
        panic!("a full team is read");
    };
    assert_eq!(found["o/big"].people().len(), MOST_TEAM_MEMBERS);
    assert_eq!(urls(&http).len(), 1 + MOST_TEAM_MEMBERS / 100);
    let groups: Vec<Value> = (0..100)
        .map(|n| json!({ "id": n, "full_path": format!("g/s{n}") }))
        .collect();
    let mut answers = vec![answer(200, &Value::Array(groups))];
    answers.extend((0..100).map(|_| answer(200, &json!([{ "username": "ann" }]))));
    let http = ScriptedHttp::new(answers);
    let gitlab = origin(Forge::GitLab, None, "g/p");
    let Ok(Teamed::Read(found)) = read(&http, &gitlab, "g") else {
        panic!("a full page of groups is read");
    };
    assert_eq!(found.len(), MOST_FORGE_TEAMS);
    assert!(found.contains_key("g") && found.contains_key("g/s98"));
    assert!(!found.contains_key("g/s99"));
    assert_eq!(urls(&http).len(), 1 + MOST_FORGE_TEAMS);
}

#[test]
fn a_read_costs_its_pages_of_teams_and_of_each_team_s_members() {
    assert_eq!(cost(Forge::GitHub), 1 + 100 * 5);
    assert_eq!(cost(Forge::GitLab), 1 + 100 * 5);
    assert_eq!(cost(Forge::Forgejo), 2 + 100 * 10);
    assert!(note(Forge::GitHub).contains(&format!("{} requests", cost(Forge::GitHub))));
    assert!(note(Forge::GitLab).contains("full path"));
    assert!(note(Forge::Forgejo).contains("org/team"));
    assert_eq!(owner_of(Forge::GitHub, "o/r"), "o");
    assert_eq!(owner_of(Forge::Forgejo, "o/r"), "o");
    assert_eq!(owner_of(Forge::GitLab, "g/s/p"), "g/s");
    assert_eq!(owner_of(Forge::GitHub, "o"), "o");
    assert_eq!(portless("gitlab.example:8443"), "gitlab.example");
    assert_eq!(portless("[::1]:8443"), "[::1]");
    assert_eq!(portless("[::1]"), "[::1]");
    assert_eq!(portless("forge.example:"), "forge.example:");
}

#[test]
fn a_repository_source_reads_the_teams_of_its_repository_s_owner() {
    let queued = Queued {
        origin: origin(
            Forge::GitLab,
            Some("https://gitlab.example/api/v4"),
            "g/s/p",
        ),
        token: None,
        project: "p".to_owned(),
        graphql: String::new(),
    };
    assert_eq!(queued.team_owner(), "https://gitlab.example/api/v4 g/s");
    let http = ScriptedHttp::new(vec![answer(404, &json!({}))]);
    assert_eq!(
        pollster::block_on(queued.teams(&http)),
        Ok(Teamed::Refused(permission(Forge::GitLab)))
    );
    assert_eq!(
        urls(&http),
        ["https://gitlab.example/api/v4/groups/g%2Fs/descendant_groups?per_page=100&page=1"]
    );
    let options = SourceOptions {
        teams: Some(crate::TeamsFrom::Forge),
        ..SourceOptions::default()
    };
    assert!(options.forge_teams());
    assert!(!SourceOptions::default().forge_teams());
    assert_eq!(crate::TeamsFrom::default(), crate::TeamsFrom::Config);
}

#[test]
fn a_source_says_it_reads_its_forge_s_teams_and_doctor_s_note_only_where_it_says_forge() {
    use herald_kinds::SourceConfig as _;
    let source = |teams: Option<crate::TeamsFrom>| crate::ForgeSource {
        forge: Forge::Forgejo,
        options: SourceOptions {
            repository: Some("o/r".to_owned()),
            teams,
            ..SourceOptions::default()
        },
        names: None,
    };
    let forge = source(Some(crate::TeamsFrom::Forge));
    assert!(forge.forge_teams());
    assert_eq!(forge.teams_note(), Some(note(Forge::Forgejo)));
    for unread in [None, Some(crate::TeamsFrom::Config)] {
        assert!(!source(unread).forge_teams());
        assert_eq!(source(unread).teams_note(), None);
    }
}
