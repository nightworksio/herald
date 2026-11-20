//! What doctor names of the ID tokens apps take: an entry binding no id a
//! renamed or re-created repository never inherits is a gap, a token taken
//! until it expires is a gap where the store cannot remember it and a note
//! where the app chose it, and a name an app stands for that no herald
//! source grants is noted.

use super::*;

const ISSUER: &str = "https://token.actions.githubusercontent.com";

#[test]
fn an_entry_binding_names_alone_is_a_gap_and_a_name_granted_nowhere_a_note() {
    let text = format!(
        r#"version = 1
[store]
kind = "git-branch"
key = {{ secret = "HERALD_STORE_KEY" }}
[federation]
domain = "herald.example"
keys = [{{ secret = "HERALD_KEY_NEW" }}]
[apps.named]
oidc = [{{ issuer = "{ISSUER}", claims = {{ repository = "org/plugin", repository_id = "*" }} }}]
peers = ["named-ci"]
[apps.subject]
oidc = [{{ issuer = "{ISSUER}", claims = {{ sub = "repo:acme/app:ref:refs/heads/main" }} }}]
peers = ["subject-ci"]
[apps.bound]
oidc = [{{ issuer = "{ISSUER}", claims = {{ repository_id = "42" }} }}, {{ issuer = "{ISSUER}", claims = {{ repository_owner_id = "7", workflow = "release" }} }}]
peers = ["bound-ci", "spare-ci"]
[sources.inbox]
kind = "herald"
peers = {{ named-ci = {{ actions = ["push"] }}, bound-ci = {{ actions = ["push"] }}, subject-ci = {{ actions = ["push"] }} }}
[destinations.releases]
kind = "herald"
[[routes]]
id = "mirror"
sources = ["inbox"]
destination = "releases"
private = false
"#
    );
    let config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let findings = doctored(&config, &layer(&STORES));
    assert!(
        findings
            .gaps
            .iter()
            .all(|gap| !gap.key.ends_with(".replay")),
        "{:?}",
        findings.gaps
    );
    let gaps: Vec<&Gap> = findings
        .gaps
        .iter()
        .filter(|gap| gap.key.starts_with("apps."))
        .collect();
    assert_eq!(
        gaps.iter().map(|gap| gap.key.as_str()).collect::<Vec<_>>(),
        ["apps.named.oidc[0].claims", "apps.subject.oidc[0].claims"]
    );
    assert_eq!(gaps[0].why, gaps[1].why);
    assert_eq!(
        gaps[0].why,
        format!(
            "the entry for {ISSUER} binds a run by names alone, such as its repository's, its owner's or its sub, and none of repository_id, repository_owner_id, project_id, namespace_id to one id, so a repository renamed or re-created under a name it matches takes its grant; bind one with no wildcard"
        )
    );
    let notes: Vec<&String> = findings
        .notes
        .iter()
        .filter(|note| note.starts_with("apps."))
        .collect();
    assert_eq!(
        notes,
        [
            "apps.bound.peers: no herald source grants \"spare-ci\" anything, so what a run of the app's pushes as it is refused; grant it in a herald source's peers, or leave it out."
        ]
    );
}

fn replaying(store: &str, replay: &str) -> Findings {
    let text = format!(
        r#"version = 1
[store]
{store}
[server]
url = "https://herald.example"
[apps.ci]
{replay}oidc = [{{ issuer = "{ISSUER}", claims = {{ repository_id = "42" }}, keys = [{{ kty = "RSA", kid = "k1", n = "AQID", e = "AQAB" }}] }}]
projects = ["herald"]
[destinations.discord]
kind = "discord"
token = {{ secret = "T" }}
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
[[routes]]
id = "ci"
sources = ["ci"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
private = false
"#
    );
    let config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    doctored(&config, &layer(&STORES))
}

#[test]
fn a_token_taken_until_it_expires_is_a_gap_where_the_store_cannot_remember_it() {
    let unkept = replaying("kind = \"markers\"", "");
    let gaps: Vec<&Gap> = unkept
        .gaps
        .iter()
        .filter(|gap| gap.key == "apps.ci.replay")
        .collect();
    assert_eq!(gaps.len(), 1, "{:?}", unkept.gaps);
    assert_eq!(gaps[0].feature, "taking each CI run's ID token once");
    assert!(
        gaps[0].why.starts_with(
            "the markers store keeps no deliveries, so a token the app takes is taken again"
        ),
        "{}",
        gaps[0].why
    );
    let chosen = replaying(
        "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }",
        "replay = \"until-expiry\"\n",
    );
    assert!(chosen.gaps.iter().all(|gap| gap.key != "apps.ci.replay"));
    assert!(
        chosen
            .notes
            .iter()
            .any(|note| note.starts_with("apps.ci.replay: a token the app takes is taken again")),
        "{:?}",
        chosen.notes
    );
    let once = replaying(
        "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }",
        "",
    );
    assert!(once.gaps.iter().all(|gap| gap.key != "apps.ci.replay"));
    assert!(
        once.notes
            .iter()
            .all(|note| !note.starts_with("apps.ci.replay"))
    );
}
