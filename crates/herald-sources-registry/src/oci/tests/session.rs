use herald_kinds::{Probed, Unpolled};
use herald_ports::{HttpError, Method};
use herald_testing::ScriptedHttp;

use super::super::session::{Blob, Failure, Reached, Session};
use super::*;

const HUB_CHALLENGE: &str = r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/caddy:pull""#;

/// The scheme and credential of `request`'s `Authorization`, where it has
/// one.
fn credential(request: &Request) -> Option<(String, String)> {
    header(request, "Authorization")?
        .split_once(' ')
        .map(|(scheme, credential)| (scheme.to_owned(), credential.to_owned()))
}

fn challenged(challenge: &str) -> Response {
    with(answer(401, ""), "WWW-Authenticate", challenge)
}

fn hub(token: Option<Secret>) -> Reached {
    let options = OciOptions {
        username: token.as_ref().map(|_| "me".to_owned()),
        ..of("https://registry-1.docker.io", "caddy")
    };
    Reached::new(&options, token)
}

fn reached(options: &OciOptions) -> Reached {
    Reached::new(options, None)
}

fn sent(reached: &Reached, http: &ScriptedHttp, path: &str) -> Result<Response, Failure> {
    let mut session = Session::new(reached, http, "repository:x:pull".to_owned());
    let request = session.get(path);
    pollster::block_on(session.send(request))
}

#[test]
fn an_anonymous_bearer_challenge_is_answered_from_the_trusted_realm_and_asked_again() {
    let reached = hub(None);
    let http = ScriptedHttp::new(vec![
        challenged(HUB_CHALLENGE),
        answer(200, r#"{"token":"anon"}"#),
        answer(200, "{}"),
        answer(200, "[]"),
    ]);
    let mut session = Session::new(&reached, &http, "repository:library/caddy:pull".to_owned());
    let request = session.get("/v2/library/caddy/tags/list");
    assert_eq!(
        pollster::block_on(session.send(request)).unwrap().status,
        200
    );
    let again = session.get("/v2/library/caddy/manifests/1");
    assert_eq!(pollster::block_on(session.send(again)).unwrap().body, b"[]");
    let requests = http.requests();
    assert_eq!(
        requests[1].url.expose(),
        "https://auth.docker.io/token?service=registry.docker.io&scope=repository%3Alibrary%2Fcaddy%3Apull"
    );
    assert_eq!(header(&requests[1], "Authorization"), None);
    assert_eq!(header(&requests[0], "Authorization"), None);
    assert_eq!(
        credential(&requests[2]),
        Some(("Bearer".to_owned(), "anon".to_owned()))
    );
    assert_eq!(
        credential(&requests[3]),
        Some(("Bearer".to_owned(), "anon".to_owned()))
    );
    assert_eq!(
        requests[2].url.expose(),
        "https://registry-1.docker.io/v2/library/caddy/tags/list"
    );
    assert!(!requests[2].url.redacted().contains("anon"));
    assert!(!format!("{:?}", requests[2]).contains("anon"));
}

#[test]
fn a_bearer_challenge_takes_the_username_and_token_to_its_realm_and_the_session_s_scope_where_it_names_none()
 {
    let reached = Reached::new(
        &OciOptions {
            username: Some("me".to_owned()),
            token: Some(token()),
            ..of("https://registry.example", "acme/app")
        },
        Some(read_token()),
    );
    let http = ScriptedHttp::new(vec![
        challenged(r#"Bearer realm="https://registry.example/token?x=1""#),
        answer(200, r#"{"access_token":"issued"}"#),
        answer(200, "{}"),
    ]);
    assert!(sent(&reached, &http, "/v2/acme/app/tags/list").is_ok());
    let requests = http.requests();
    assert_eq!(
        requests[1].url.expose(),
        "https://registry.example/token?x=1&scope=repository%3Ax%3Apull"
    );
    assert_eq!(
        header(&requests[1], "Authorization").as_deref(),
        Some(format!("Basic {}", herald_core::basic("me", "secret")).as_str())
    );
    assert_eq!(
        credential(&requests[2]),
        Some(("Bearer".to_owned(), "issued".to_owned()))
    );
    assert!(!format!("{:?}", requests[1]).contains(&herald_core::basic("me", "secret")));
}

#[test]
fn a_realm_neither_on_the_registry_nor_on_auth_s_origin_is_refused_at_auth() {
    let reached = reached(&of("https://registry.example", "acme/app"));
    for (realm, shown) in [
        (
            "https://evil.example/token?account=x",
            "https://evil.example",
        ),
        ("not a url", "an address that is not a web URL"),
        (
            "ftp://registry.example/t",
            "an address that is not a web URL",
        ),
        (
            "https://me:pw@registry.example/token",
            "an address that is not a web URL",
        ),
    ] {
        let http = ScriptedHttp::new(vec![challenged(&format!(r#"Bearer realm="{realm}""#))]);
        assert_eq!(
            sent(&reached, &http, "/v2/").unwrap_err(),
            Failure::at(
                "auth",
                format!(
                    "the registry sends its tokens from {shown}, which is neither its own origin nor auth's; set auth to that service's base URL where it is the registry's own"
                )
            )
        );
        assert_eq!(http.requests().len(), 1);
    }
    let trusting = super::super::session::Reached::new(
        &OciOptions {
            auth: Some("https://gitlab.example".to_owned()),
            ..of("https://registry.gitlab.example", "acme/app")
        },
        None,
    );
    let http = ScriptedHttp::new(vec![
        challenged(
            r#"Bearer realm="https://gitlab.example/jwt/auth",service="container_registry""#,
        ),
        answer(200, r#"{"token":"t"}"#),
        answer(200, "{}"),
    ]);
    assert!(sent(&trusting, &http, "/v2/").is_ok());
}

#[test]
fn a_token_service_refusing_or_failing_says_which_key_to_look_at() {
    let reached = hub(None);
    let refused = ScriptedHttp::new(vec![challenged(HUB_CHALLENGE), answer(401, "")]);
    assert_eq!(
        sent(&reached, &refused, "/v2/").unwrap_err(),
        Failure::at(
            "token",
            "the token service at https://auth.docker.io answered 401; check username and token"
        )
    );
    let failing = ScriptedHttp::new(vec![challenged(HUB_CHALLENGE), answer(500, "")]);
    assert_eq!(sent(&reached, &failing, "/v2/").unwrap_err().key, "auth");
    let tokenless = ScriptedHttp::new(vec![challenged(HUB_CHALLENGE), answer(200, "{}")]);
    assert_eq!(
        sent(&reached, &tokenless, "/v2/").unwrap_err(),
        Failure::at(
            "auth",
            "the registry's token could not be read: it issues no token"
        )
    );
    let silent = ScriptedHttp::new(vec![challenged(HUB_CHALLENGE)]);
    let failure = sent(&reached, &silent, "/v2/").unwrap_err();
    assert_eq!(failure.key, "auth");
    assert!(failure.unreachable);
}

#[test]
fn a_credential_refused_after_its_challenge_is_answered_says_to_give_one_that_reads() {
    let reached = hub(None);
    let http = ScriptedHttp::new(vec![
        challenged(HUB_CHALLENGE),
        answer(200, r#"{"token":"anon"}"#),
        answer(403, ""),
    ]);
    assert_eq!(
        sent(&reached, &http, "/v2/").unwrap_err(),
        Failure::at(
            "token",
            "the registry answered 403 with the credential its challenge asked for; give a username and token that may read repository:x:pull"
        )
    );
}

#[test]
fn a_basic_challenge_takes_the_username_and_token_and_without_them_says_to_give_them() {
    let options = OciOptions {
        username: Some("me".to_owned()),
        token: Some(token()),
        ..of("https://registry.example", "acme/app")
    };
    let reached = Reached::new(&options, Some(read_token()));
    let http = ScriptedHttp::new(vec![
        challenged(r#"Basic realm="Registry""#),
        answer(200, "{}"),
    ]);
    assert!(sent(&reached, &http, "/v2/").is_ok());
    assert_eq!(
        header(&http.requests()[1], "Authorization").as_deref(),
        Some(format!("Basic {}", herald_core::basic("me", "secret")).as_str())
    );
    let anonymous = Reached::new(&of("https://registry.example", "acme/app"), None);
    let http = ScriptedHttp::new(vec![challenged(r#"Basic realm="Registry""#)]);
    assert_eq!(
        sent(&anonymous, &http, "/v2/").unwrap_err(),
        Failure::at(
            "token",
            "the registry asks for a username and password; give username and token"
        )
    );
}

#[test]
fn an_answer_without_a_challenge_herald_answers_is_handed_back() {
    let reached = hub(None);
    for unanswered in [
        answer(401, ""),
        challenged(r#"Digest realm="r""#),
        answer(404, ""),
    ] {
        let status = unanswered.status;
        let http = ScriptedHttp::new(vec![unanswered]);
        assert_eq!(sent(&reached, &http, "/v2/").unwrap().status, status);
        assert_eq!(http.requests().len(), 1);
    }
    let silent = ScriptedHttp::new(Vec::new());
    assert_eq!(
        sent(&reached, &silent, "/v2/").unwrap_err(),
        Failure {
            key: "registry",
            why: HttpError::TimedOut.to_string(),
            unreachable: true,
        }
    );
}

fn blob(reached: &Reached, http: &ScriptedHttp) -> Result<Blob, Failure> {
    let mut session = Session::new(reached, http, "repository:x:pull".to_owned());
    let request = session.get("/v2/x/blobs/sha256:a");
    pollster::block_on(session.blob(request))
}

fn redirect(location: &str) -> Response {
    with(answer(307, ""), "Location", location)
}

#[test]
fn a_blob_redirected_to_a_cdn_blobs_names_is_fetched_without_the_registry_s_credential() {
    let reached = hub(None);
    let http = ScriptedHttp::new(vec![
        challenged(HUB_CHALLENGE),
        answer(200, r#"{"token":"anon"}"#),
        redirect("https://production.cloudfront.docker.com/blobs/a?sig=s3cret"),
        answer(200, r#"{"created":"2025-01-01T00:00:00Z"}"#),
    ]);
    let Blob::Read(read) = blob(&reached, &http).unwrap() else {
        panic!("the blob is read");
    };
    assert_eq!(read.status, 200);
    let requests = http.requests();
    assert_eq!(requests[3].method, Method::Get);
    assert_eq!(
        requests[3].url.expose(),
        "https://production.cloudfront.docker.com/blobs/a?sig=s3cret"
    );
    assert!(!requests[3].url.redacted().contains("s3cret"));
    assert_eq!(header(&requests[3], "Authorization"), None);
    assert!(header(&requests[3], "User-Agent").is_some());
}

#[test]
fn a_blob_redirected_on_the_registry_is_asked_with_its_credential() {
    let reached = hub(None);
    let http = ScriptedHttp::new(vec![
        challenged(HUB_CHALLENGE),
        answer(200, r#"{"token":"anon"}"#),
        redirect("/storage/a"),
        answer(200, "{}"),
    ]);
    assert!(matches!(blob(&reached, &http).unwrap(), Blob::Read(_)));
    let requests = http.requests();
    assert_eq!(
        requests[3].url.expose(),
        "https://registry-1.docker.io/storage/a"
    );
    assert_eq!(
        credential(&requests[3]),
        Some(("Bearer".to_owned(), "anon".to_owned()))
    );
}

#[test]
fn a_blob_redirected_elsewhere_is_not_followed_and_names_where() {
    let reached = reached(&of("https://registry.example", "acme/app"));
    let unparsed = "an address that is not a web URL";
    for (location, named) in [
        (
            "https://cdn.example/a?Signature=s3cret",
            "https://cdn.example",
        ),
        ("//cdn.example/a?Signature=s3cret", unparsed),
        ("relative/a?Signature=s3cret", unparsed),
        ("https://me:s3cret@cdn.example/a", unparsed),
    ] {
        let http = ScriptedHttp::new(vec![redirect(location)]);
        let Blob::Redirected(origin) = blob(&reached, &http).unwrap() else {
            panic!("{location} is not followed");
        };
        assert_eq!(origin, named);
        assert!(!origin.contains("s3cret"), "{origin}");
        assert_eq!(http.requests().len(), 1);
    }
    let http = ScriptedHttp::new(vec![answer(302, "")]);
    assert!(matches!(
        blob(&reached, &http).unwrap(),
        Blob::Redirected(origin) if origin == unparsed
    ));
    for status in [301, 303, 308] {
        let http = ScriptedHttp::new(vec![with(
            answer(status, ""),
            "Location",
            "https://cdn.example/a",
        )]);
        assert!(matches!(
            blob(&reached, &http).unwrap(),
            Blob::Redirected(_)
        ));
    }
    let http = ScriptedHttp::new(vec![answer(404, "")]);
    assert!(matches!(blob(&reached, &http).unwrap(), Blob::Read(read) if read.status == 404));
}

#[test]
fn a_cdn_that_cannot_be_reached_fails_at_blobs() {
    let options = OciOptions {
        blobs: Some(vec!["https://cdn.example/".to_owned()]),
        ..of("https://registry.example", "acme/app")
    };
    let reached = Reached::new(&options, None);
    let http = ScriptedHttp::new(vec![redirect("https://cdn.example/a")]);
    let failure = blob(&reached, &http).err().unwrap();
    assert_eq!(failure.key, "blobs");
    assert!(failure.unreachable);
    assert_eq!(
        reached.bases,
        ["https://registry.example", "https://cdn.example"]
    );
}

#[test]
fn a_failure_is_unreachable_or_unreadable_and_a_doctor_s_finding_at_its_key() {
    let unreachable = Failure {
        key: "registry",
        why: "down".to_owned(),
        unreachable: true,
    };
    assert_eq!(
        Unpolled::from(unreachable.clone()),
        Unpolled::Unreachable("down".to_owned())
    );
    assert_eq!(
        Unpolled::from(Failure::at("auth", "refused")),
        Unpolled::Unreadable("refused".to_owned())
    );
    assert_eq!(
        Probed::from(unreachable),
        Probed {
            key: "registry",
            why: "down".to_owned()
        }
    );
}
