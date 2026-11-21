use herald_core::{Permission, SecretRef};
use herald_kinds::{Granted, Polled, SourceConfig as _};
use herald_ports::{Method, Response, Secret};
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::{ForgeSource, SignIn as SignInOptions, SourceOptions, Write};

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn ok(body: &Value) -> Response {
    answer(200, body)
}

fn scoped(scopes: &str, body: &Value) -> Response {
    Response {
        headers: vec![("x-oauth-scopes".to_owned(), scopes.to_owned())],
        ..ok(body)
    }
}

fn job(run: &str, job: &str) -> Rerun {
    Rerun {
        run: run.to_owned(),
        job: job.to_owned(),
        page: String::new(),
        call: String::new(),
    }
}

fn listed(retried: usize, plain: usize) -> Response {
    let jobs: Vec<Value> = (0..retried)
        .map(|_| json!({ "name": "test", "retried": true }))
        .chain((0..plain).map(|_| json!({ "name": "lint" })))
        .collect();
    ok(&json!(jobs))
}

fn options(api: Option<&str>) -> SourceOptions {
    SourceOptions {
        repository: Some("o/r".to_owned()),
        api: api.map(ToOwned::to_owned),
        token: Some(SecretRef {
            secret: "HERALD_READ".to_owned(),
        }),
        write: Some(Write {
            token: Some(SecretRef {
                secret: "HERALD_RERUN".to_owned(),
            }),
            app: None,
            installation: None,
        }),
        sign_in: Some(SignInOptions {
            client_id: "Iv23liClient".to_owned(),
            scopes: None,
            web: None,
            least: None,
        }),
        ..SourceOptions::default()
    }
}

fn rerunner(forge: Forge, options: &SourceOptions) -> ForgeRerunner {
    ForgeRerunner {
        forge,
        api: crate::forge::api_of(forge, options),
        parse: 1 << 20,
        read: Some(Token::Named(Secret::new("HERALD_READ", "read-value"))),
        credential: "HERALD_RERUN".to_owned(),
        signing: options.signing(forge),
    }
}

fn github() -> ForgeRerunner {
    rerunner(Forge::GitHub, &options(None))
}

fn gitlab() -> ForgeRerunner {
    rerunner(
        Forge::GitLab,
        &options(Some("https://gitlab.example/api/v4")),
    )
}

fn oauth() -> OperatorToken {
    OperatorToken::new(format!("{GITHUB_OAUTH_TOKEN}operator-value"))
}

fn app_user() -> OperatorToken {
    OperatorToken::new(format!("{GITHUB_APP_USER_TOKEN}operator-value"))
}

fn bearer(request: &Request) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.expose())
}

#[test]
fn the_sign_in_is_asked_at_each_forge_s_web_root_with_the_narrowest_scope() {
    let cases = [
        (Forge::GitHub, None, "https://github.com", ""),
        (
            Forge::GitHub,
            Some("https://api.acme.ghe.com"),
            "https://acme.ghe.com",
            "",
        ),
        (
            Forge::GitHub,
            Some("https://ghe.example/api/v3/"),
            "https://ghe.example",
            "",
        ),
        (
            Forge::GitLab,
            Some("https://gitlab.example/api/v4"),
            "https://gitlab.example",
            "read_api",
        ),
    ];
    for (forge, api, web, scope) in cases {
        let signing = options(api).signing(forge).unwrap();
        assert_eq!(signing.web, web, "{api:?}");
        assert_eq!(signing.scopes.join(" "), scope);
        assert_eq!(signing.least, Permission::Write);
    }
    let mut set = options(None);
    set.sign_in = Some(SignInOptions {
        client_id: "c".to_owned(),
        scopes: Some(vec!["read:org".to_owned()]),
        web: Some("https://login.example/".to_owned()),
        least: Some(Permission::Maintain),
    });
    let signing = set.signing(Forge::GitHub).unwrap();
    assert_eq!(
        (signing.web.as_str(), signing.scopes, signing.least),
        (
            "https://login.example",
            vec!["read:org".to_owned()],
            Permission::Maintain
        )
    );
    assert_eq!(
        options(None).signing(Forge::Forgejo).unwrap_err(),
        crate::FORGEJO_UNWRITTEN
    );
    let mut unsigned = options(None);
    unsigned.sign_in = None;
    assert_eq!(
        unsigned.signing(Forge::GitHub).unwrap_err(),
        herald_kinds::UNSIGNED
    );
}

#[test]
fn a_device_code_is_asked_for_and_each_answer_of_the_wait_is_told_apart() {
    for (rerunner, start, token, page) in [
        (
            github(),
            "https://github.com/login/device/code",
            "https://github.com/login/oauth/access_token",
            "https://github.com/login/device",
        ),
        (
            gitlab(),
            "https://gitlab.example/oauth/authorize_device",
            "https://gitlab.example/oauth/token",
            "https://gitlab.example/oauth/device",
        ),
    ] {
        let http = ScriptedHttp::new(vec![
            ok(&json!({
                "device_code": "device-value", "user_code": "WDJB-MJHT",
                "verification_uri": page
            })),
            ok(&json!({ "error": "authorization_pending" })),
            answer(400, &json!({ "error": "slow_down" })),
            ok(&json!({ "error": "expired_token" })),
            ok(&json!({ "error": "access_denied" })),
            ok(&json!({ "error": "unsupported_grant_type" })),
            ok(&json!({ "access_token": "operator-value" })),
        ]);
        let device = pollster::block_on(rerunner.device(&http)).unwrap();
        assert_eq!(device.user_code, "WDJB-MJHT");
        assert_eq!(device.interval, INTERVAL);
        assert_eq!(device.expires, EXPIRES);
        assert!(!format!("{device:?}").contains("device-value"));
        let mut said = Vec::new();
        for _ in 0..6 {
            said.push(
                match pollster::block_on(rerunner.polled(&http, &device)).unwrap() {
                    Polled::Pending => "pending".to_owned(),
                    Polled::Slower => "slower".to_owned(),
                    Polled::Signed(token) => token.sent().expose().to_owned(),
                    Polled::Ended(reason) => reason,
                },
            );
        }
        assert_eq!(
            said,
            [
                "pending",
                "slower",
                EXPIRED,
                DENIED,
                ENDED,
                "operator-value"
            ]
        );
        let requests = http.requests();
        assert_eq!(requests[0].url.expose(), start);
        assert_eq!(requests[0].method, Method::Post);
        assert!(!requests[0].sealed);
        let scope = if rerunner.forge == Forge::GitLab {
            "read_api"
        } else {
            ""
        };
        assert_eq!(
            String::from_utf8(requests[0].body.clone()).unwrap(),
            format!("client_id=Iv23liClient&scope={scope}")
        );
        assert_eq!(requests[1].url.expose(), token);
        assert!(requests[1].sealed);
        assert!(!format!("{:?}", requests[1]).contains("device-value"));
        assert!(requests.iter().all(|request| bearer(request).is_none()));
    }
}

#[test]
fn a_sign_in_page_off_the_web_root_or_over_plain_http_is_never_shown() {
    for page in [
        "https://elsewhere.example/login/device",
        "http://github.com/login/device",
        "https://github.com.elsewhere.example/login/device",
        "not a page",
    ] {
        let http = ScriptedHttp::new(vec![ok(&json!({
            "device_code": "device-value", "user_code": "WDJB-MJHT",
            "verification_uri": page
        }))]);
        let refused = pollster::block_on(github().device(&http));
        assert!(
            matches!(refused, Err(Unpolled::Unreadable(ref reason)) if reason == ELSEWHERE),
            "{page}"
        );
    }
}

#[test]
fn an_operator_s_token_holding_a_scope_that_writes_is_refused() {
    let token = OperatorToken::new("operator-value");
    let login = json!({ "login": "octocat" });
    for (scopes, held) in [
        ("", true),
        ("read:org, read:user", true),
        ("read:org, repo", false),
        ("workflow", false),
    ] {
        let http = ScriptedHttp::new(vec![scoped(scopes, &login)]);
        let named = pollster::block_on(github().operator((&http, &http), &oauth(), None));
        assert_eq!(named.is_ok(), held, "{scopes}: {named:?}");
    }
    let gitlab = gitlab();
    for (scope, held) in [
        (json!(["read_api", "openid"]), true),
        (json!(["api"]), false),
    ] {
        let http = ScriptedHttp::new(vec![
            ok(&json!({ "username": "maintainer" })),
            ok(&json!({ "scope": scope, "expires_in": 7200 })),
        ]);
        let named = pollster::block_on(gitlab.operator((&http, &http), &token, None));
        assert_eq!(named.is_ok(), held, "{scope}: {named:?}");
        let requests = http.requests();
        assert_eq!(
            requests[1].url.expose(),
            "https://gitlab.example/oauth/token/info"
        );
        assert_eq!(
            bearer(&requests[1]).as_deref(),
            Some("Bearer operator-value")
        );
    }
}

#[test]
fn a_gitlab_token_is_revoked_as_a_public_client_and_a_github_one_cannot_be() {
    let token = OperatorToken::new("operator-value");
    let http = ScriptedHttp::new(vec![ok(&json!({}))]);
    assert!(!pollster::block_on(github().revoked(&http, &token)).unwrap());
    assert_eq!(http.requests().len(), 0);
    assert!(pollster::block_on(gitlab().revoked(&http, &token)).unwrap());
    let requests = http.requests();
    assert_eq!(requests[0].method, Method::Post);
    assert_eq!(
        requests[0].url.expose(),
        "https://gitlab.example/oauth/revoke"
    );
    assert!(requests[0].sealed && bearer(&requests[0]).is_none());
    assert_eq!(
        String::from_utf8(requests[0].body.clone()).unwrap(),
        "client_id=Iv23liClient&token=operator-value"
    );
    let refused = ScriptedHttp::new(vec![answer(400, &json!({}))]);
    assert!(pollster::block_on(gitlab().revoked(&refused, &token)).is_err());
}

#[test]
fn the_operator_is_named_and_their_role_read_with_their_own_token() {
    let token = app_user();
    let http = ScriptedHttp::new(vec![
        ok(&json!({ "login": "octocat" })),
        installations(&[reading(1, "Iv23liClient")]),
        app(&json!({ "metadata": "read", "emails": "read" })),
        ok(&json!({ "permissions": { "push": true } })),
        ok(&json!({ "permissions": { "pull": true } })),
        ok(&json!({ "permissions": { "admin": true } })),
    ]);
    let rerunner = github();
    let login = pollster::block_on(rerunner.operator((&http, &http), &token, None)).unwrap();
    assert_eq!(login, "octocat");
    let granted = |http: &ScriptedHttp| pollster::block_on(rerunner.granted(http, &token, "o/r"));
    assert_eq!(granted(&http).unwrap(), Granted::Enough);
    assert_eq!(
        granted(&http).unwrap(),
        Granted::Short("read or none".to_owned())
    );
    assert_eq!(granted(&http).unwrap(), Granted::Enough);
    let requests = http.requests();
    assert_eq!(requests[0].url.expose(), "https://api.github.com/user");
    assert_eq!(
        requests[1].url.expose(),
        "https://api.github.com/user/installations?per_page=100&page=1"
    );
    assert_eq!(
        requests[2].url.expose(),
        "https://api.github.com/apps/herald-sign-in"
    );
    assert_eq!(requests[3].url.expose(), "https://api.github.com/repos/o/r");
    let sent = format!("Bearer {}", token.sent().expose());
    assert!(
        requests
            .iter()
            .all(|request| bearer(request).as_deref() == Some(sent.as_str()))
    );
    let mut maintain = options(None);
    maintain.sign_in.as_mut().unwrap().least = Some(Permission::Maintain);
    let strict = self::rerunner(Forge::GitHub, &maintain);
    let http = ScriptedHttp::new(vec![ok(&json!({ "permissions": { "push": true } }))]);
    assert_eq!(
        pollster::block_on(strict.granted(&http, &token, "o/r")).unwrap(),
        Granted::Short("write".to_owned())
    );
}

#[test]
fn a_gitlab_operator_holds_the_higher_of_their_project_and_group_access() {
    let token = OperatorToken::new("operator-value");
    let rerunner = gitlab();
    let http = ScriptedHttp::new(vec![
        ok(&json!({ "username": "maintainer" })),
        ok(&json!({ "scope": ["read_api"] })),
        ok(
            &json!({ "permissions": { "project_access": { "access_level": 20 }, "group_access": { "access_level": 30 } } }),
        ),
        ok(
            &json!({ "permissions": { "project_access": { "access_level": 20 }, "group_access": null } }),
        ),
        ok(&json!({ "id": 1 })),
    ]);
    assert_eq!(
        pollster::block_on(rerunner.operator((&http, &http), &token, None)).unwrap(),
        "maintainer"
    );
    let granted = || pollster::block_on(rerunner.granted(&http, &token, "g/p")).unwrap();
    assert_eq!(granted(), Granted::Enough);
    assert_eq!(granted(), Granted::Short("access level 20".to_owned()));
    assert_eq!(granted(), Granted::Short("access level 0".to_owned()));
    assert_eq!(
        http.requests()[2].url.expose(),
        "https://gitlab.example/api/v4/projects/g%2Fp"
    );
}

#[test]
fn a_pull_request_s_head_and_a_job_are_read_with_the_read_credential() {
    let rerunner = github();
    let failed =
        json!({ "run_id": 7, "head_sha": "abc", "status": "completed", "conclusion": "failure" });
    let running =
        json!({ "run_id": 7, "head_sha": "abc", "status": "in_progress", "conclusion": null });
    let http = ScriptedHttp::new(vec![
        ok(&json!({ "state": "open", "head": { "sha": "abc" } })),
        ok(&json!({ "state": "closed", "head": { "sha": "abc" } })),
        ok(&failed),
        ok(&json!({ "run_attempt": 1 })),
        ok(&running),
        ok(&json!({ "run_attempt": 2 })),
        ok(
            &json!({ "run_id": 8, "head_sha": "abc", "status": "completed", "conclusion": "failure" }),
        ),
    ]);
    let head = || pollster::block_on(rerunner.head(&http, ("o/r", 5))).unwrap();
    assert_eq!(head().as_deref(), Some("abc"));
    assert_eq!(head(), None);
    let planned = job("7", "70");
    let run = || pollster::block_on(rerunner.run(&http, ("o/r", &planned)));
    assert_eq!(
        run().unwrap(),
        RunNow {
            head: "abc".to_owned(),
            failed: true,
            reruns: 0
        }
    );
    assert_eq!(
        run().unwrap(),
        RunNow {
            head: "abc".to_owned(),
            failed: false,
            reruns: 1
        }
    );
    assert!(
        matches!(run(), Err(Unpolled::Unreadable(ref reason)) if reason.contains("not of the run 7"))
    );
    let requests = http.requests();
    assert_eq!(
        requests[0].url.expose(),
        "https://api.github.com/repos/o/r/pulls/5"
    );
    assert_eq!(
        requests[2].url.expose(),
        "https://api.github.com/repos/o/r/actions/jobs/70"
    );
    assert_eq!(
        requests[3].url.expose(),
        "https://api.github.com/repos/o/r/actions/runs/7"
    );
    assert_eq!(requests.len(), 7);
    assert!(
        requests
            .iter()
            .all(|request| bearer(request).as_deref() == Some("Bearer read-value"))
    );
}

#[test]
fn a_gitlab_job_counts_the_retried_jobs_of_its_name_on_every_page_of_its_pipeline() {
    let rerunner = gitlab();
    let found =
        json!({ "name": "test", "status": "failed", "pipeline": { "id": 7, "sha": "abc" } });
    let http = ScriptedHttp::new(vec![
        ok(&json!({ "state": "opened", "sha": "abc" })),
        ok(&json!({ "state": "merged", "sha": "abc" })),
        ok(&found),
        listed(1, 99),
        listed(1, 0),
        ok(&json!({ "name": "test", "status": "failed", "pipeline": { "id": 6, "sha": "abc" } })),
    ]);
    let head = || pollster::block_on(rerunner.head(&http, ("g/p", 5))).unwrap();
    assert_eq!(head().as_deref(), Some("abc"));
    assert_eq!(head(), None);
    let planned = job("7", "70");
    let run = pollster::block_on(rerunner.run(&http, ("g/p", &planned))).unwrap();
    assert_eq!(
        run,
        RunNow {
            head: "abc".to_owned(),
            failed: true,
            reruns: 2
        }
    );
    let other = pollster::block_on(rerunner.run(&http, ("g/p", &planned)));
    assert!(
        matches!(other, Err(Unpolled::Unreadable(ref reason)) if reason.contains("not of the run 7"))
    );
    let urls: Vec<String> = http
        .requests()
        .iter()
        .map(|request| request.url.expose())
        .collect();
    assert_eq!(
        urls,
        [
            "https://gitlab.example/api/v4/projects/g%2Fp/merge_requests/5",
            "https://gitlab.example/api/v4/projects/g%2Fp/merge_requests/5",
            "https://gitlab.example/api/v4/projects/g%2Fp/jobs/70",
            "https://gitlab.example/api/v4/projects/g%2Fp/pipelines/7/jobs?include_retried=true&per_page=100&page=1",
            "https://gitlab.example/api/v4/projects/g%2Fp/pipelines/7/jobs?include_retried=true&per_page=100&page=2",
            "https://gitlab.example/api/v4/projects/g%2Fp/jobs/70",
        ]
    );
    let mut endless = vec![ok(&found)];
    endless.extend((0..MOST_JOB_PAGES).map(|_| listed(0, PAGE)));
    let http = ScriptedHttp::new(endless);
    let unknown = pollster::block_on(rerunner.run(&http, ("g/p", &planned)));
    assert!(matches!(unknown, Err(Unpolled::Unreadable(ref reason)) if reason == UNCOUNTED));
    assert_eq!(http.requests().len(), 1 + MOST_JOB_PAGES);
}

#[test]
fn the_one_job_is_re_run_with_the_write_credential_alone_and_only_a_numbered_job_is_named() {
    for (rerunner, url) in [
        (
            github(),
            "https://api.github.com/repos/o/r/actions/jobs/70/rerun",
        ),
        (
            gitlab(),
            "https://gitlab.example/api/v4/projects/o%2Fr/jobs/70/retry",
        ),
    ] {
        let written = || Credential::Named(Secret::new("HERALD_RERUN", "write-value"));
        let http = ScriptedHttp::new(vec![answer(201, &json!({}))]);
        let answered =
            pollster::block_on(rerunner.rerun(&http, written(), ("o/r", &job("7", "70")))).unwrap();
        assert_eq!(answered.unwrap().status, 201);
        let requests = http.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, Method::Post);
        assert_eq!(requests[0].url.expose(), url);
        assert_eq!(bearer(&requests[0]).as_deref(), Some("Bearer write-value"));
        for unnumbered in ["", "7/../8", "..", "7a"] {
            let refused = pollster::block_on(rerunner.rerun(
                &http,
                written(),
                ("o/r", &job("7", unnumbered)),
            ));
            assert!(
                matches!(refused, Err(Unpolled::Unreadable(ref reason)) if reason == UNNUMBERED),
                "{unnumbered}"
            );
            let read = pollster::block_on(rerunner.run(&http, ("o/r", &job(unnumbered, "70"))));
            assert!(read.is_err(), "{unnumbered}");
        }
        let dotted = pollster::block_on(rerunner.run(&http, ("o/..", &job("7", "70"))));
        assert!(dotted.is_err());
        assert_eq!(http.requests().len(), 1);
    }
}

#[test]
fn a_source_holds_its_write_credential_apart_from_its_read_credential() {
    let source = |options: SourceOptions| ForgeSource {
        forge: Forge::GitHub,
        options,
        names: None,
    };
    let held = source(options(None));
    assert_eq!(
        held.write().map(|write| write.name().to_owned()).as_deref(),
        Some("HERALD_RERUN")
    );
    assert!(
        !held
            .secret_refs()
            .iter()
            .any(|named| named.secret == "HERALD_RERUN")
    );
    let built = held.rerunner(None).unwrap();
    assert_eq!(built.credential(), "HERALD_RERUN");
    assert_eq!(built.api(), "https://api.github.com");
    assert_eq!(built.signing().unwrap().web, "https://github.com");
    let forgejo = ForgeSource {
        forge: Forge::Forgejo,
        options: SourceOptions::default(),
        names: None,
    };
    assert_eq!(
        forgejo.rerunner(None).unwrap_err(),
        crate::FORGEJO_UNWRITTEN
    );
    let mut unwritten = options(None);
    unwritten.write = None;
    assert_eq!(
        source(unwritten).rerunner(None).unwrap_err(),
        herald_kinds::UNWRITTEN
    );
}

#[test]
fn a_forge_s_interval_and_expiry_are_taken_above_zero_and_a_form_encodes_each_value() {
    assert_eq!(seconds(None, INTERVAL), INTERVAL);
    assert_eq!(seconds(Some(0), INTERVAL), INTERVAL);
    assert_eq!(seconds(Some(-3), EXPIRES), EXPIRES);
    assert_eq!(seconds(Some(10), INTERVAL), SignedDuration::from_secs(10));
    assert_eq!(form(&[("a", "b c"), ("d", "e&f")]), "a=b%20c&d=e%26f");
}

#[test]
fn a_rerunner_with_no_sign_in_asks_nothing_and_a_forge_naming_no_login_is_refused() {
    let mut unsigned = github();
    unsigned.signing = Err(herald_kinds::UNSIGNED);
    let http = ScriptedHttp::new(vec![ok(&json!({ "id": 1 }))]);
    assert!(pollster::block_on(unsigned.device(&http)).is_err());
    let device = Device {
        user_code: "c".to_owned(),
        verification: "v".to_owned(),
        device_code: Secret::new("the device code", "device-value"),
        interval: INTERVAL,
        expires: EXPIRES,
    };
    assert!(pollster::block_on(unsigned.polled(&http, &device)).is_err());
    let token = OperatorToken::new("operator-value");
    assert!(pollster::block_on(unsigned.granted(&http, &token, "o/r")).is_err());
    assert!(pollster::block_on(unsigned.revoked(&http, &token)).is_ok());
    assert_eq!(http.requests().len(), 0);
    assert_eq!(unsigned.signing().unwrap_err(), herald_kinds::UNSIGNED);
    let unnamed = ScriptedHttp::new(vec![scoped("", &json!({ "id": 1 }))]);
    let nameless = pollster::block_on(github().operator((&unnamed, &unnamed), &oauth(), None));
    assert!(
        matches!(nameless, Err(Unpolled::Unreadable(ref reason)) if reason.contains("no login")),
        "{nameless:?}"
    );
}

#[test]
fn a_github_role_is_read_from_its_highest_flag_and_a_job_is_failed_only_once_completed() {
    let token = OperatorToken::new("operator-value");
    let mut maintain = options(None);
    maintain.sign_in.as_mut().unwrap().least = Some(Permission::Maintain);
    let strict = rerunner(Forge::GitHub, &maintain);
    let attempt = json!({ "run_attempt": 1 });
    let http = ScriptedHttp::new(vec![
        ok(&json!({ "permissions": { "maintain": true, "push": true } })),
        ok(
            &json!({ "run_id": 7, "head_sha": "abc", "status": "completed", "conclusion": "success" }),
        ),
        ok(&attempt),
        ok(&json!({ "run_id": 7, "head_sha": "abc", "status": "queued", "conclusion": "failure" })),
        ok(&attempt),
    ]);
    assert_eq!(
        pollster::block_on(strict.granted(&http, &token, "o/r")).unwrap(),
        Granted::Enough
    );
    let planned = job("7", "70");
    let run = || pollster::block_on(strict.run(&http, ("o/r", &planned))).unwrap();
    assert_eq!(
        run(),
        RunNow {
            head: "abc".to_owned(),
            failed: false,
            reruns: 0
        }
    );
    assert!(!run().failed);
}

#[test]
fn an_app_writes_only_for_a_repository_or_an_owner() {
    let app = Write {
        token: None,
        app: Some("rerun".to_owned()),
        installation: None,
    };
    let owned = SourceOptions {
        owner: Some("o".to_owned()),
        write: Some(app.clone()),
        ..SourceOptions::default()
    };
    assert!(matches!(
        owned.writing(Forge::GitHub),
        Some(herald_kinds::WriteCredential::App(installed))
            if installed.installation == herald_kinds::Installation::Owner("o")
    ));
    let nowhere = SourceOptions {
        write: Some(app),
        ..SourceOptions::default()
    };
    assert_eq!(nowhere.writing(Forge::GitHub), None);
}

fn installations(listed: &[Value]) -> Response {
    ok(&json!({ "total_count": listed.len(), "installations": listed }))
}

fn reading(app_id: u64, client_id: &str) -> Value {
    json!({ "app_id": app_id, "app_slug": "herald-sign-in", "client_id": client_id,
        "permissions": { "metadata": "read", "pull_requests": "read" } })
}

fn app(permissions: &Value) -> Response {
    ok(&json!({ "id": 1, "slug": "herald-sign-in", "permissions": permissions }))
}

fn reading_app() -> Response {
    app(&json!({ "metadata": "read", "pull_requests": "read", "emails": "read" }))
}

fn signed_in_as_app(answers: Vec<Response>, writer: Option<&str>) -> Result<String, Unpolled> {
    let answered = [vec![ok(&json!({ "login": "octocat" }))], answers].concat();
    let http = ScriptedHttp::new(answered);
    pollster::block_on(github().operator((&http, &http), &app_user(), writer))
}

#[test]
fn an_app_s_user_token_is_refused_where_an_installation_it_reaches_can_write_or_is_the_write_app() {
    let read = reading(1, "Iv23liClient");
    assert_eq!(
        signed_in_as_app(
            vec![installations(std::slice::from_ref(&read)), reading_app()],
            Some("12345")
        )
        .unwrap(),
        "octocat"
    );
    let mut writing = read.clone();
    writing["permissions"]["contents"] = json!("write");
    let mut administering = read.clone();
    administering["permissions"]["administration"] = json!("admin");
    for (listed, writer, said) in [
        (vec![read.clone(), writing], None, "contents: write"),
        (vec![administering], None, "administration: admin"),
        (
            vec![reading(12345, "Iv23liOther")],
            Some("12345"),
            WRITER_REACHED,
        ),
        (
            vec![reading(9, "Iv23liRerun")],
            Some("Iv23liRerun"),
            WRITER_REACHED,
        ),
    ] {
        let refused = signed_in_as_app(vec![installations(&listed)], writer);
        assert!(
            matches!(refused, Err(Unpolled::Unreadable(ref reason)) if reason.contains(said)),
            "{said}: {refused:?}"
        );
    }
}

#[test]
fn an_app_s_user_token_is_read_on_every_page_and_refused_where_they_cannot_be_read_whole() {
    let full: Vec<Value> = (0..100).map(|id| reading(id, "Iv23liClient")).collect();
    let mut writing = reading(7, "Iv23liClient");
    writing["permissions"]["actions"] = json!("write");
    let paged = vec![installations(&full), installations(&[writing])];
    assert!(signed_in_as_app(paged, None).is_err());
    let http = ScriptedHttp::new(vec![
        ok(&json!({ "login": "octocat" })),
        installations(&full),
        installations(&[]),
        reading_app(),
    ]);
    assert!(pollster::block_on(github().operator((&http, &http), &app_user(), None)).is_ok());
    let urls: Vec<String> = http
        .requests()
        .iter()
        .map(|request| request.url.expose())
        .collect();
    assert_eq!(
        urls[2..],
        [
            "https://api.github.com/user/installations?per_page=100&page=2",
            "https://api.github.com/apps/herald-sign-in"
        ]
    );
    let endless = vec![installations(&full); MOST_INSTALLATION_PAGES];
    for unread in [
        vec![answer(404, &json!({}))],
        vec![ok(&json!({ "installed": [] }))],
        endless,
    ] {
        let refused = signed_in_as_app(unread, None);
        assert!(
            matches!(refused, Err(Unpolled::Unreadable(ref reason)) if reason.starts_with(UNINSTALLED)),
            "{refused:?}"
        );
    }
}

/// How a GitHub token's sign-in ended, by its scopes `header`, where the
/// installations it reaches answer `installations` and its App answers
/// `app`: its login, or why it was refused; and how many requests it made.
fn signed_in_with(
    token: &OperatorToken,
    header: Option<&str>,
    (installations, app): (Option<Response>, Option<Response>),
) -> (Result<String, Unpolled>, usize) {
    let login = json!({ "login": "octocat" });
    let user = header.map_or_else(|| ok(&login), |header| scoped(header, &login));
    let answers = [Some(user), installations, app]
        .into_iter()
        .flatten()
        .collect();
    let http = ScriptedHttp::new(answers);
    let named = pollster::block_on(github().operator((&http, &http), token, None));
    (named, http.requests().len())
}

#[test]
fn a_github_token_is_held_to_the_rule_of_its_prefix_and_no_failed_read_lets_it_in() {
    let headers = [None, Some(""), Some("read:org"), Some("repo")];
    let readable = || {
        (
            Some(installations(&[reading(1, "Iv23liClient")])),
            Some(reading_app()),
        )
    };
    let unread = [
        (Some(answer(500, &json!({}))), None),
        (Some(answer(403, &json!({ "message": "rate limit" }))), None),
        (None, None),
        (Some(ok(&json!({ "installed": [] }))), None),
        (
            Some(installations(&[reading(1, "Iv23liClient")])),
            Some(answer(500, &json!({}))),
        ),
        (Some(installations(&[reading(1, "Iv23liClient")])), None),
    ];
    for header in headers {
        let named = header.is_some_and(|scopes| scopes != "repo");
        let (signed, asked) = signed_in_with(&oauth(), header, readable());
        assert_eq!(signed.is_ok(), named, "OAuth, {header:?}: {signed:?}");
        assert_eq!(asked, 1, "an OAuth token's installations are never read");
        if header.is_none() {
            assert!(matches!(signed, Err(Unpolled::Unreadable(ref reason)) if reason == UNSCOPED));
        }
        let (signed, _) = signed_in_with(&app_user(), header, readable());
        assert_eq!(
            signed.is_ok(),
            header != Some("repo"),
            "App, {header:?}: {signed:?}"
        );
        for failed in unread.clone() {
            let (signed, _) = signed_in_with(&app_user(), header, failed);
            assert!(
                matches!(signed, Err(Unpolled::Unreadable(ref reason)) if reason.starts_with(UNINSTALLED)),
                "App, {header:?}: {signed:?}"
            );
        }
        for other in [
            OperatorToken::new("operator-value"),
            OperatorToken::new(["ghs", "_operator-value"].concat()),
            OperatorToken::new(["github", "_pat_operator-value"].concat()),
            OperatorToken::new(["x", GITHUB_OAUTH_TOKEN, "operator-value"].concat()),
            OperatorToken::new(["x", GITHUB_APP_USER_TOKEN, "operator-value"].concat()),
        ] {
            let (signed, asked) = signed_in_with(&other, header, readable());
            assert!(
                matches!(signed, Err(Unpolled::Unreadable(ref reason)) if reason == UNKNOWN_TOKEN),
                "{header:?}: {signed:?}"
            );
            assert_eq!(asked, 1);
            let shown = format!("{signed:?}");
            assert!(
                !shown.contains("operator-value")
                    && !shown.contains("ghs")
                    && !shown.contains("pat")
            );
        }
    }
}

#[test]
fn an_app_s_user_token_reaching_another_app_s_installation_is_refused_without_a_request() {
    let foreign = signed_in_as_app(vec![installations(&[reading(1, "Iv23liForeign")])], None);
    assert!(
        matches!(foreign, Err(Unpolled::Unreadable(ref reason)) if reason == FOREIGN_APP),
        "{foreign:?}"
    );
    let mut unnamed = reading(1, "Iv23liClient");
    unnamed["client_id"] = Value::Null;
    let listed = vec![installations(&[unnamed]), reading_app()];
    assert_eq!(signed_in_as_app(listed, None).unwrap(), "octocat");
}

#[test]
fn an_app_s_user_token_is_refused_where_its_app_holds_more_than_read_or_is_named_by_none() {
    let read = reading(1, "Iv23liClient");
    let mut unnamed = read.clone();
    unnamed["app_slug"] = Value::Null;
    for (answers, said) in [
        (
            vec![
                installations(std::slice::from_ref(&read)),
                app(&json!({ "metadata": "read", "git_ssh_keys": "write" })),
            ],
            "git_ssh_keys: write",
        ),
        (
            vec![
                installations(std::slice::from_ref(&read)),
                app(&json!({ "metadata": "read", "gpg_keys": "admin" })),
            ],
            "gpg_keys: admin",
        ),
        (vec![installations(&[])], UNNAMED_APP),
        (vec![installations(&[unnamed])], UNNAMED_APP),
        (
            vec![
                installations(std::slice::from_ref(&read)),
                ok(&json!({ "id": 1 })),
            ],
            UNINSTALLED,
        ),
    ] {
        let refused = signed_in_as_app(answers, None);
        assert!(
            matches!(refused, Err(Unpolled::Unreadable(ref reason)) if reason.contains(said)),
            "{said}: {refused:?}"
        );
    }
    for dotted in ["..", "."] {
        let mut stepping = read.clone();
        stepping["app_slug"] = json!(dotted);
        let http = ScriptedHttp::new(vec![
            ok(&json!({ "login": "octocat" })),
            installations(&[stepping]),
            reading_app(),
        ]);
        let refused = pollster::block_on(github().operator((&http, &http), &app_user(), None));
        assert!(
            matches!(refused, Err(Unpolled::Unreadable(ref reason)) if reason.contains("App's slug")),
            "{dotted}: {refused:?}"
        );
        assert_eq!(http.requests().len(), 2, "{dotted}");
    }
    let mut odd = reading(2, "Iv23liClient");
    odd["app_slug"] = json!("sign in/../x");
    let http = ScriptedHttp::new(vec![
        ok(&json!({ "login": "octocat" })),
        installations(&[read.clone(), read, odd]),
        reading_app(),
        reading_app(),
    ]);
    assert!(pollster::block_on(github().operator((&http, &http), &app_user(), None)).is_ok());
    let urls: Vec<String> = http
        .requests()
        .iter()
        .map(|request| request.url.expose())
        .collect();
    assert_eq!(
        urls[2..],
        [
            "https://api.github.com/apps/herald-sign-in",
            "https://api.github.com/apps/sign%20in%2F..%2Fx"
        ]
    );
}

#[test]
fn a_page_is_on_the_web_root_only_over_https_at_its_own_origin() {
    assert!(on_web(
        "https://github.com/login/device",
        "https://github.com"
    ));
    assert!(!on_web(
        "http://ghes.example/login/device",
        "http://ghes.example"
    ));
    assert!(!on_web("not a page", "not a root"));
    assert!(!on_web(
        "https://evil.example/login/device",
        "https://github.com"
    ));
}

#[test]
fn a_gitlab_job_counts_only_the_retried_jobs_of_its_own_name() {
    let found =
        json!({ "name": "test", "status": "failed", "pipeline": { "id": 7, "sha": "abc" } });
    let others = ok(&json!([{ "name": "lint", "retried": true }, { "name": "test" }]));
    let http = ScriptedHttp::new(vec![ok(&found), others]);
    let run = pollster::block_on(gitlab().run(&http, ("g/p", &job("7", "70")))).unwrap();
    assert_eq!(run.reruns, 0);
}

#[test]
fn an_app_s_user_token_reaching_an_installation_naming_an_empty_app_is_refused_unread() {
    let mut unnamed = reading(1, "Iv23liClient");
    unnamed["app_slug"] = json!("");
    let refused = signed_in_as_app(vec![installations(&[unnamed])], None);
    assert!(
        matches!(refused, Err(Unpolled::Unreadable(ref reason)) if reason == UNNAMED_APP),
        "{refused:?}"
    );
}
