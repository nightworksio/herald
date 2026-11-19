//! `[federation.pages]`: the static signed log, served on `[federation]`'s
//! domain, signed by its keys, kept in a store that keeps records, and
//! committed to a branch or written to a directory.

use herald_config::{Config, Pages, Uses, load};
use herald_core::SecretRef;
use jiff::SignedDuration;

fn loaded(store: &str, federation: &str) -> Result<Config, String> {
    let text = format!("version = 1\n\n[store]\n{store}\n{federation}");
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::common::kinds(),
    )
    .map_err(|errors| errors.to_string())
}

fn federated(pages: &str) -> Result<Config, String> {
    loaded(
        "kind = \"sqlite\"",
        &format!(
            "[federation]\ndomain = \"owner.example\"\nkeys = [{{ secret = \"HERALD_KEY\" }}]\n\n[federation.pages]\n{pages}\n"
        ),
    )
}

#[test]
fn a_static_log_is_committed_to_herald_pages_and_its_path_is_the_instance_s() {
    let config = federated("url = \"https://owner.example/log/\"").unwrap();
    assert_eq!(
        config.federation.pages,
        Some(Pages {
            url: "https://owner.example/log".to_owned(),
            base: "/log".to_owned(),
            branch: Some(Pages::BRANCH.to_owned()),
            repository: None,
            token: None,
            api: None,
            artifact: None,
            refresh: Pages::REFRESH,
        })
    );
    assert_eq!(config.instance_base(), "/log");
    for uses in [Uses::Announces, Uses::Polls, Uses::Serves] {
        let used = config.secrets_used(uses);
        assert!(used.contains("HERALD_KEY"), "{uses:?}");
        assert_eq!(
            used.contains("GITHUB_TOKEN"),
            uses != Uses::Serves,
            "{uses:?}"
        );
    }
    let unlogged = loaded(
        "kind = \"sqlite\"",
        "[federation]\ndomain = \"owner.example\"\nkeys = [{ secret = \"HERALD_KEY\" }]\n",
    )
    .unwrap();
    assert_eq!(unlogged.instance_base(), "");
    assert!(!unlogged.secrets_used(Uses::Polls).contains("HERALD_KEY"));
}

#[test]
fn every_setting_is_taken_as_written_and_an_artifact_alone_takes_the_branch_s_place() {
    let config = federated(
            "url = \"https://owner.example\"\nbranch = \"gh/pages\"\nrepository = \"owner/site\"\ntoken = { secret = \"PAGES_TOKEN\" }\napi = \"https://git.example/api/v3\"\nartifact = \"_site\"\nrefresh = \"7d\"",
        )
        .unwrap();
    let pages = config.federation.pages.clone().unwrap();
    assert_eq!(pages.base, "");
    assert_eq!(pages.branch.as_deref(), Some("gh/pages"));
    assert_eq!(pages.repository.as_deref(), Some("owner/site"));
    assert_eq!(
        pages.token,
        Some(SecretRef {
            secret: "PAGES_TOKEN".to_owned()
        })
    );
    assert_eq!(pages.api.as_deref(), Some("https://git.example/api/v3"));
    assert_eq!(pages.artifact.as_deref(), Some("_site"));
    assert_eq!(pages.refresh, SignedDuration::from_hours(7 * 24));
    assert!(config.secrets_used(Uses::Polls).contains("PAGES_TOKEN"));
    let artifact = federated(
            "url = \"https://owner.example\"\nartifact = \"_site\"\ntoken = { secret = \"PAGES_TOKEN\" }",
        )
        .unwrap();
    let pages = artifact.federation.pages.clone().unwrap();
    assert_eq!(pages.branch, None);
    let used = artifact.secrets_used(Uses::Polls);
    assert!(!used.contains("PAGES_TOKEN") && !used.contains("GITHUB_TOKEN"));
}

#[test]
fn a_log_served_elsewhere_unsigned_or_kept_in_markers_is_refused() {
    for (written, said) in [
        (
            "url = \"http://owner.example\"",
            "federation.pages.url: \"http://owner.example\" is not where a static host serves the log; give https://, the domain and a path, such as \"https://owner.github.io/repository\"",
        ),
        (
            "url = \"https://other.example\"",
            "federation.pages.url: the log is served on other.example, but [federation] domain is owner.example; serve it on the domain its keys are published on",
        ),
        (
            "url = \"https://owner.example\"\nrefresh = \"8d\"",
            "federation.pages.refresh: pages.refresh is at most \"7d\"; shorten it",
        ),
    ] {
        let refused = federated(written).unwrap_err();
        assert!(refused.contains(said), "{refused}");
    }
    for branch in ["", "-a", ".a", "/a", "a/", "a.", "a..b", "a//b", "a b"] {
        let refused = federated(&format!(
            "url = \"https://owner.example\"\nbranch = \"{branch}\""
        ))
        .unwrap_err();
        assert!(
                refused.contains(&format!("federation.pages.branch: \"{branch}\" is not a branch's name; give letters, digits, '-', '_', '.' and '/', such as \"herald-pages\"")),
                "{refused}"
            );
    }
    let unsigned = loaded(
        "kind = \"sqlite\"",
        "[federation]\n\n[federation.pages]\nurl = \"https://owner.example\"\n",
    )
    .unwrap_err();
    assert!(
            unsigned.contains("federation.pages.url: the log is served on owner.example, but [federation] domain is not given"),
            "{unsigned}"
        );
    assert!(
            unsigned.contains("federation.pages: the static log is signed by [federation]'s newest key; give keys in [federation]"),
            "{unsigned}"
        );
    let markers = loaded(
            "kind = \"markers\"",
            "[federation]\ndomain = \"owner.example\"\nkeys = [{ secret = \"HERALD_KEY\" }]\n\n[federation.pages]\nurl = \"https://owner.example\"\n",
        )
        .unwrap_err();
    assert!(
            markers.contains("federation.pages: the markers store keeps what was announced in the posts it reads back, and the static log's sequence is kept in none; give a store that keeps records, such as \"git-branch\""),
            "{markers}"
        );
}
