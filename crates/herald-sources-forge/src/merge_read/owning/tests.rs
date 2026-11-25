use herald_core::merging::Method;
use herald_kinds::{MergeReader, Methods};
use herald_testing::ScriptedHttp;
use serde_json::json;

use super::super::tests::{ok, reader, refused};
use crate::{Forge, Origin, SourceOptions};

fn commit(verified: bool, email: &str) -> herald_ports::Response {
    ok(&json!({
        "parents": [{ "sha": "h1" }, { "sha": "b1" }],
        "commit": { "committer": { "email": email }, "verification": { "verified": verified } },
    }))
}

fn made(forge: Forge, answers: Vec<herald_ports::Response>) -> (ScriptedHttp, Option<Vec<String>>) {
    let http = ScriptedHttp::new(answers);
    let made = pollster::block_on(reader(forge).forge_made(&http, ("h2", None))).unwrap();
    (http, made)
}

fn methods(forge: Forge, settings: &serde_json::Value) -> Methods {
    let http = ScriptedHttp::new(vec![ok(settings)]);
    pollster::block_on(reader(forge).methods(&http)).unwrap()
}

#[test]
fn a_commit_github_made_and_signed_gives_its_parents_and_any_other_none() {
    let (http, parents) = made(Forge::GitHub, vec![commit(true, "noreply@github.com")]);
    assert_eq!(parents, Some(vec!["h1".to_owned(), "b1".to_owned()]));
    assert_eq!(
        http.requests()[0].url.expose(),
        "https://api.github.com/repos/o/r/commits/h2"
    );
    let (_, parents) = made(Forge::GitHub, vec![commit(false, "noreply@github.com")]);
    assert_eq!(parents, None);
    let (_, parents) = made(Forge::GitHub, vec![commit(true, "author@example.com")]);
    assert_eq!(parents, None);
    for forge in [Forge::GitLab, Forge::Forgejo] {
        let (http, parents) = made(forge, vec![]);
        assert_eq!(parents, None);
        assert_eq!(http.requests().len(), 0);
    }
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        api: Some("https://ghe.example/api/v3".to_owned()),
        ..SourceOptions::default()
    };
    let mut server = reader(Forge::GitHub);
    server.origin = Origin::new(Forge::GitHub, &options, "o/r");
    let http = ScriptedHttp::new(vec![commit(true, "noreply@github.com")]);
    assert_eq!(
        pollster::block_on(server.forge_made(&http, ("h2", None))).unwrap(),
        None
    );
    assert_eq!(http.requests().len(), 0);
    let signer = Some("web-flow@ghe.example");
    let http = ScriptedHttp::new(vec![commit(true, "web-flow@ghe.example")]);
    let parents = pollster::block_on(server.forge_made(&http, ("h2", signer))).unwrap();
    assert_eq!(parents, Some(vec!["h1".to_owned(), "b1".to_owned()]));
    assert_eq!(
        http.requests()[0].url.expose(),
        "https://ghe.example/api/v3/repos/o/r/commits/h2"
    );
    let http = ScriptedHttp::new(vec![commit(true, "noreply@github.com")]);
    assert_eq!(
        pollster::block_on(server.forge_made(&http, ("h2", signer))).unwrap(),
        None
    );
    let http = ScriptedHttp::new(vec![commit(false, "web-flow@ghe.example")]);
    assert_eq!(
        pollster::block_on(server.forge_made(&http, ("h2", signer))).unwrap(),
        None
    );
    let http = ScriptedHttp::new(vec![]);
    let gitlab = pollster::block_on(reader(Forge::GitLab).forge_made(&http, ("h2", signer)));
    assert_eq!(gitlab.unwrap(), None);
    assert_eq!(http.requests().len(), 0);
    let http = ScriptedHttp::new(vec![refused(404)]);
    assert!(pollster::block_on(reader(Forge::GitHub).forge_made(&http, ("h2", None))).is_err());
    let http = ScriptedHttp::new(vec![]);
    assert!(pollster::block_on(reader(Forge::GitHub).forge_made(&http, ("../x", None))).is_err());
    assert_eq!(http.requests().len(), 0);
}

#[test]
fn each_forge_says_whether_a_pull_request_is_merged() {
    for (forge, body, path) in [
        (
            Forge::GitHub,
            json!({ "merged": true }),
            "/repos/o/r/pulls/7",
        ),
        (
            Forge::Forgejo,
            json!({ "merged": true }),
            "/repos/o/r/pulls/7",
        ),
        (
            Forge::GitLab,
            json!({ "state": "merged" }),
            "/merge_requests/7",
        ),
    ] {
        let http = ScriptedHttp::new(vec![ok(&body)]);
        assert!(pollster::block_on(reader(forge).merged(&http, 7)).unwrap());
        let url = http.requests()[0].url.expose().clone();
        assert!(url.ends_with(path), "{url}");
        let http = ScriptedHttp::new(vec![ok(&json!({ "state": "opened", "merged": false }))]);
        assert!(!pollster::block_on(reader(forge).merged(&http, 7)).unwrap());
    }
}

#[test]
fn each_forge_says_the_methods_it_allows_and_whether_it_said_each() {
    let github = methods(
        Forge::GitHub,
        &json!({ "allow_merge_commit": false, "allow_squash_merge": true, "allow_rebase_merge": true }),
    );
    assert_eq!(github.allowed, [Method::Squash, Method::Rebase]);
    assert!(github.whole);
    let hidden = methods(Forge::GitHub, &json!({ "allow_squash_merge": true }));
    assert_eq!(hidden.allowed, [Method::Squash]);
    assert!(!hidden.whole);
    let forgejo = methods(
        Forge::Forgejo,
        &json!({ "allow_merge_commits": true, "allow_squash_merge": false, "allow_rebase": false, "allow_fast_forward_only_merge": true }),
    );
    assert_eq!(forgejo.allowed, [Method::Merge, Method::FastForward]);
    assert!(forgejo.whole);
    for (merge_method, method) in [
        ("merge", Method::Merge),
        ("rebase_merge", Method::Rebase),
        ("ff", Method::FastForward),
    ] {
        let gitlab = methods(
            Forge::GitLab,
            &json!({ "merge_method": merge_method, "squash_option": "default_off" }),
        );
        assert_eq!(gitlab.allowed, [method, Method::Squash]);
        assert!(gitlab.whole);
    }
    let never = methods(
        Forge::GitLab,
        &json!({ "merge_method": "merge", "squash_option": "never" }),
    );
    assert_eq!(never.allowed, [Method::Merge]);
    assert!(never.whole);
    let unknown = methods(Forge::GitLab, &json!({ "merge_method": "other" }));
    assert_eq!(unknown.allowed, []);
    assert!(!unknown.whole);
    let unsaid = methods(Forge::GitLab, &json!({ "merge_method": "merge" }));
    assert!(!unsaid.whole);
}
