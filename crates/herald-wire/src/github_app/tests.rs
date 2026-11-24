use herald_ports::{HttpError, Response};
use serde_json::{Value, json};

use super::*;
use herald_testing::ScriptedHttp;

const API: &str = "https://api.github.com";

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn token() -> Value {
    json!({ "token": "ghs_minted", "expires_at": "2026-11-16T13:00:00Z" })
}

fn jwt_of() -> Secret {
    Secret::new("JWT", "header.claims.signature")
}

fn mint(
    scripted: &ScriptedHttp,
    installation: Installation<'_>,
    known: Option<u64>,
) -> Result<InstallationToken, Unminted> {
    pollster::block_on(minted(
        scripted,
        (API, installation, known),
        ("herald", &jwt_of()),
    ))
}

#[test]
fn an_app_asserts_itself_with_an_rs256_jwt_issued_a_minute_back_for_nine_minutes() {
    let now: Timestamp = "2026-11-16T12:00:00Z".parse().unwrap();
    let assertion = assertion("Iv23liABCDEF", now);
    let (header, claims) = assertion.split_once('.').unwrap();
    assert_eq!(URL_SAFE_NO_PAD.decode(header).unwrap(), HEADER.as_bytes());
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).unwrap()).unwrap();
    assert_eq!(
        claims,
        json!({
            "iat": now.as_second() - 60,
            "exp": now.as_second() + 540,
            "iss": "Iv23liABCDEF",
        })
    );
    let signed = jwt("JWT", &assertion, &[0xfb, 0xff]);
    assert_eq!(signed.expose(), format!("{assertion}.-_8"));
    assert_eq!(signed.name(), "JWT");
}

#[test]
fn an_owner_s_installation_is_looked_up_as_an_organization_then_a_user_and_minted() {
    let scripted = ScriptedHttp::new(vec![
        answer(404, &json!({})),
        answer(200, &json!({ "id": 42 })),
        answer(201, &token()),
    ]);
    let minted = mint(&scripted, Installation::Owner("wessel"), None).unwrap();
    assert_eq!(minted.token.expose(), "ghs_minted");
    assert_eq!(minted.token.name(), "herald");
    assert_eq!(minted.installation, 42);
    assert_eq!(
        minted.expires,
        "2026-11-16T13:00:00Z".parse::<Timestamp>().unwrap()
    );
    let asked: Vec<String> = scripted.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        asked,
        [
            format!("{API}/orgs/wessel/installation"),
            format!("{API}/users/wessel/installation"),
            format!("{API}/app/installations/42/access_tokens"),
        ]
    );
    let requests = scripted.requests();
    assert_eq!(requests[2].method, Method::Post);
    assert!(requests.iter().all(|request| {
        request.headers.iter().any(|(name, value)| {
            name == "Authorization"
                && value.expose().split_once(' ') == Some(("Bearer", "header.claims.signature"))
        })
    }));
}

#[test]
fn a_known_or_named_installation_is_minted_at_once_and_a_repository_s_looked_up() {
    let scripted = ScriptedHttp::new(vec![answer(201, &token())]);
    assert_eq!(
        mint(&scripted, Installation::Owner("o"), Some(7))
            .unwrap()
            .installation,
        7
    );
    let scripted = ScriptedHttp::new(vec![answer(201, &token())]);
    assert_eq!(
        mint(&scripted, Installation::Fixed(9), None)
            .unwrap()
            .installation,
        9
    );
    assert_eq!(
        scripted.asked()[0].0,
        format!("{API}/app/installations/9/access_tokens")
    );
    let scripted = ScriptedHttp::new(vec![
        answer(200, &json!({ "id": 3 })),
        answer(201, &token()),
    ]);
    mint(&scripted, Installation::Repository("cli/cli"), None).unwrap();
    assert_eq!(
        scripted.asked()[0].0,
        format!("{API}/repos/cli/cli/installation")
    );
}

#[test]
fn an_app_installed_nowhere_or_refused_mints_nothing_and_says_why() {
    let scripted = ScriptedHttp::new(vec![answer(404, &json!({})), answer(404, &json!({}))]);
    assert_eq!(
        mint(&scripted, Installation::Owner("o"), None),
        Err(Unminted::unreadable(&SourceError::Unreadable {
            what: INSTALLATION,
            reason: "the App is not installed there; install it on the owner or the repository"
                .to_owned(),
        }))
    );
    let scripted = ScriptedHttp::new(vec![answer(401, &json!({}))]);
    assert_eq!(
        mint(&scripted, Installation::Repository("o/r"), None),
        Err(Unminted::unreadable(&SourceError::Answered {
            what: INSTALLATION,
            status: 401
        }))
    );
    let scripted = ScriptedHttp::new(vec![answer(200, &json!({ "id": "x" }))]);
    assert!(mint(&scripted, Installation::Repository("o/r"), None).is_err());
    let scripted = ScriptedHttp::new(vec![answer(403, &json!({}))]);
    assert_eq!(
        mint(&scripted, Installation::Fixed(1), None),
        Err(Unminted::unreadable(&SourceError::Answered {
            what: MINTED,
            status: 403
        }))
    );
    let scripted = ScriptedHttp::new(vec![answer(201, &json!({ "token": "t" }))]);
    assert!(mint(&scripted, Installation::Fixed(1), None).is_err());
    let scripted = ScriptedHttp::new(Vec::new());
    assert_eq!(
        mint(&scripted, Installation::Fixed(1), None),
        Err(Unminted::Unreachable(HttpError::TimedOut.to_string()))
    );
    let scripted = ScriptedHttp::new(Vec::new());
    assert_eq!(
        mint(&scripted, Installation::Owner("o"), None),
        Err(Unminted::Unreachable(HttpError::TimedOut.to_string()))
    );
    assert_eq!(Unminted::Unreachable("down".to_owned()).to_string(), "down");
    assert_eq!(Unminted::Unreadable("odd".to_owned()).to_string(), "odd");
}

fn narrowed_token(permissions: &Value, repositories: &Value) -> Value {
    let mut answer = token();
    answer["permissions"] = permissions.clone();
    answer["repositories"] = repositories.clone();
    answer
}

/// What a narrowed mint answered `answered` minted, or why not.
type Narrowing = Result<Result<InstallationToken, Wide>, Unminted>;

fn mint_narrowed(answered: &Value) -> (ScriptedHttp, Narrowing) {
    let scripted = ScriptedHttp::new(vec![answer(201, answered)]);
    let minted = pollster::block_on(minted_narrowed(
        &scripted,
        (API, Installation::Fixed(9), None),
        ("herald", &jwt_of()),
        &rerunning("r"),
    ));
    (scripted, minted)
}

#[test]
fn a_narrowed_token_names_its_permissions_and_repositories_and_a_whole_one_sends_no_body() {
    let narrowed = rerunning("r");
    assert_eq!(
        serde_json::from_str::<Value>(narrowed.body()).unwrap(),
        json!({ "permissions": { "actions": "write" }, "repositories": ["r"] })
    );
    let (scripted, held) = mint_narrowed(&narrowed_token(
        &json!({ "actions": "write", "metadata": "read" }),
        &json!([{ "name": "r", "full_name": "o/r" }]),
    ));
    assert_eq!(held.unwrap().unwrap().installation, 9);
    let request = &scripted.requests()[0];
    assert_eq!(request.body, narrowed.body().as_bytes());
    assert!(request.headers.iter().any(|(name, value)| {
        name == "Content-Type" && value.expose() == herald_core::media::JSON
    }));
    let scripted = ScriptedHttp::new(vec![answer(201, &token())]);
    mint(&scripted, Installation::Fixed(9), None).unwrap();
    assert_eq!(scripted.requests()[0].body, b"");
}

#[test]
fn a_narrowed_token_holding_more_than_re_running_on_the_one_repository_is_refused() {
    let one = json!([{ "name": "r" }]);
    let actions = json!({ "actions": "write" });
    assert!(matches!(
        mint_narrowed(&narrowed_token(&actions, &one)).1,
        Ok(Ok(_))
    ));
    for (permissions, repositories) in [
        (
            json!({ "actions": "write", "contents": "write" }),
            one.clone(),
        ),
        (
            json!({ "actions": "write", "metadata": "write" }),
            one.clone(),
        ),
        (json!({ "actions": "read" }), one.clone()),
        (json!({ "metadata": "read" }), one.clone()),
        (json!({}), one.clone()),
        (actions.clone(), json!([{ "name": "r" }, { "name": "s" }])),
        (actions.clone(), json!([{ "name": "s" }])),
        (actions.clone(), json!([])),
    ] {
        let (scripted, refused) = mint_narrowed(&narrowed_token(&permissions, &repositories));
        assert!(
            matches!(refused, Ok(Err(ref wide @ Wide(ref token, _))) if token.expose() == "ghs_minted" && wide.to_string().contains("holds other than actions: write") && !format!("{wide:?}").contains("ghs_minted")),
            "{permissions} {repositories}: {refused:?}"
        );
        assert_eq!(scripted.requests().len(), 1);
    }
    let (_, unnamed) = mint_narrowed(&token());
    assert!(matches!(unnamed, Ok(Err(_))));
}

#[test]
fn a_token_is_revoked_with_itself_and_a_failed_revocation_is_said() {
    for (status, revoked_now) in [(204, true), (500, false)] {
        let wide = Wide(
            Secret::new("github_apps.rerun", "ghs_minted"),
            RERUN_PERMISSIONS,
        );
        let scripted = ScriptedHttp::new(vec![answer(status, &json!({}))]);
        let answered = pollster::block_on(wide.revoked(&scripted, API));
        assert_eq!(answered.is_ok(), revoked_now, "{status}: {answered:?}");
        let requests = scripted.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, Method::Delete);
        assert_eq!(
            requests[0].url.expose(),
            format!("{API}/installation/token")
        );
        let bearer: Vec<String> = requests[0]
            .headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.expose())
            .collect();
        assert_eq!(bearer, ["Bearer ghs_minted"]);
    }
}

#[test]
fn an_installation_s_app_is_named_by_its_lookup_or_asked_for_by_its_id() {
    let scripted = ScriptedHttp::new(vec![
        answer(200, &json!({ "id": 3, "app_id": 30 })),
        answer(201, &token()),
    ]);
    let minted = mint(&scripted, Installation::Repository("o/r"), None).unwrap();
    assert_eq!((minted.installation, minted.app), (3, Some(30)));
    let scripted = ScriptedHttp::new(vec![answer(201, &token())]);
    assert_eq!(
        mint(&scripted, Installation::Fixed(9), None).unwrap().app,
        None
    );
    let scripted = ScriptedHttp::new(vec![
        answer(200, &json!({ "id": 9, "app_id": 90 })),
        answer(200, &json!({ "id": 9 })),
        answer(404, &json!({})),
    ]);
    let erased = Erased(&scripted as &dyn DynHttp);
    let app = |id| pollster::block_on(app_of(&erased, (API, id), &jwt_of()));
    assert_eq!(app(9), Ok(90));
    assert!(matches!(app(9), Err(Unminted::Unreadable(ref reason)) if reason.contains("no App")));
    assert!(app(9).is_err());
    assert_eq!(scripted.asked()[0].0, format!("{API}/app/installations/9"));
}

#[test]
fn an_app_lookup_or_a_revocation_that_reaches_nothing_or_reads_nothing_says_so() {
    let unreached = ScriptedHttp::new(Vec::new());
    let erased = Erased(&unreached as &dyn DynHttp);
    assert_eq!(
        pollster::block_on(app_of(&erased, (API, 9), &jwt_of())),
        Err(Unminted::Unreachable(HttpError::TimedOut.to_string()))
    );
    let token = Secret::new("github_apps.rerun", "ghs_minted");
    assert_eq!(
        pollster::block_on(revoked(&unreached, API, &token)),
        Err(Unminted::Unreachable(HttpError::TimedOut.to_string()))
    );
    let unread = ScriptedHttp::new(vec![answer(200, &json!({ "id": "x" }))]);
    let erased = Erased(&unread as &dyn DynHttp);
    assert!(matches!(
        pollster::block_on(app_of(&erased, (API, 9), &jwt_of())),
        Err(Unminted::Unreadable(_))
    ));
}

#[test]
fn a_hand_off_s_token_holds_contents_and_pull_requests_write_on_the_one_repository_alone() {
    let narrowed = handing_off("r");
    assert_eq!(
        serde_json::from_str::<Value>(narrowed.body()).unwrap(),
        json!({ "permissions": { "contents": "write", "pull_requests": "write" }, "repositories": ["r"] })
    );
    let one = json!([{ "name": "r" }]);
    let both = json!({ "contents": "write", "pull_requests": "write", "metadata": "read" });
    let held = |permissions: &Value, repositories: &Value| {
        let scripted = ScriptedHttp::new(vec![answer(
            201,
            &narrowed_token(permissions, repositories),
        )]);
        pollster::block_on(minted_narrowed(
            &scripted,
            (API, Installation::Fixed(9), None),
            ("herald", &jwt_of()),
            &narrowed,
        ))
    };
    assert!(matches!(held(&both, &one), Ok(Ok(_))));
    for (permissions, repositories) in [
        (json!({ "contents": "write" }), one.clone()),
        (json!({ "pull_requests": "write" }), one.clone()),
        (
            json!({ "contents": "write", "pull_requests": "read" }),
            one.clone(),
        ),
        (
            json!({ "contents": "write", "pull_requests": "write", "actions": "write" }),
            one.clone(),
        ),
        (both.clone(), json!([{ "name": "s" }])),
    ] {
        let refused = held(&permissions, &repositories);
        assert!(
            matches!(refused, Ok(Err(ref wide)) if wide.to_string().contains("holds other than contents: write and pull_requests: write")),
            "{permissions} {repositories}: {refused:?}"
        );
    }
}
