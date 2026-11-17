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
