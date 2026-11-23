//! A view's `release`: the selectors that pick its pull requests, how they
//! combine, and what holds it back, every mistake named by key
//! (ADR-0081).

use herald_config::{Config, load};
use herald_core::{
    BLOCKER_LABELS, Blocker, Combine, Glob, MOST_BLOCKER_LABELS, Release, ReleasePull,
};

const EXAMPLE: &str = r#"version = 1

[store]
kind = "sqlite"

[sources.herald]
kind = "github"
repository = "nightworksio/herald"
"#;

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn viewed(release: &str) -> String {
    format!("{EXAMPLE}\n[views.ship]\nrelease = {release}\n")
}

fn loaded(release: &str) -> Release {
    let config: Config = load(
        "herald.toml",
        &viewed(release),
        &nothing,
        &crate::common::kinds(),
    )
    .unwrap();
    config.views["ship"].view.release.clone().unwrap()
}

fn errors(release: &str) -> String {
    load(
        "herald.toml",
        &viewed(release),
        &nothing,
        &crate::common::kinds(),
    )
    .unwrap_err()
    .to_string()
}

fn globs(texts: &[&str]) -> Vec<Glob> {
    texts.iter().map(|text| Glob::new(text).unwrap()).collect()
}

#[test]
fn one_milestone_is_a_release_with_every_blocker_and_the_shipped_labels() {
    assert_eq!(
        loaded("{ milestone = \"v2.*\" }"),
        Release {
            milestone: Some(Glob::new("v2.*").unwrap()),
            label: None,
            branch: None,
            pull: None,
            combine: Combine::All,
            blocker_labels: globs(&BLOCKER_LABELS),
            blocked_by: Blocker::ALL.to_vec(),
        }
    );
}

#[test]
fn every_selector_combine_labels_and_blockers_are_read() {
    let release = loaded(
        "{ label = \"ship\", branch = \"release/*\", pull = { title = \"chore: release *\" }, combine = \"any\", blocker_labels = [\"stop\"], blocked_by = [\"draft\", \"failed\"] }",
    );
    assert_eq!(
        release,
        Release {
            milestone: None,
            label: Some(Glob::new("ship").unwrap()),
            branch: Some(Glob::new("release/*").unwrap()),
            pull: Some(ReleasePull::Title(Glob::new("chore: release *").unwrap())),
            combine: Combine::Any,
            blocker_labels: globs(&["stop"]),
            blocked_by: vec![Blocker::Draft, Blocker::Failed],
        }
    );
    let all =
        loaded("{ milestone = \"v2\", combine = \"all\", blocked_by = [\"label\", \"changes\"] }");
    assert_eq!(all.combine, Combine::All);
    assert_eq!(all.blocked_by, [Blocker::Label, Blocker::Changes]);
}

#[test]
fn a_release_pull_request_is_named_by_its_tool_or_its_head_branch() {
    assert_eq!(
        loaded("{ pull = \"release-please\" }").pull,
        Some(ReleasePull::ReleasePlease)
    );
    assert_eq!(
        loaded("{ pull = \"changesets\" }").pull,
        Some(ReleasePull::Changesets)
    );
    assert_eq!(
        loaded("{ pull = { head = \"releases/next\" } }").pull,
        Some(ReleasePull::Head(Glob::new("releases/next").unwrap()))
    );
}

#[test]
fn a_view_with_no_release_has_none() {
    let config: Config = load(
        "herald.toml",
        &format!("{EXAMPLE}\n[views.ship]\n"),
        &nothing,
        &crate::common::kinds(),
    )
    .unwrap();
    assert_eq!(config.views["ship"].view.release, None);
}

#[test]
fn a_release_picking_nothing_past_its_labels_or_with_an_unread_glob_is_refused() {
    let error = errors("{ blocked_by = [\"draft\"] }");
    assert!(
        error.contains("views.ship.release: a release picks its pull requests by milestone, label, branch or pull, at least one"),
        "{error}"
    );
    let labels: Vec<String> = (0..=MOST_BLOCKER_LABELS)
        .map(|n| format!("\"l{n}\""))
        .collect();
    let error = errors(&format!(
        "{{ milestone = \"v2\", blocker_labels = [{}] }}",
        labels.join(", ")
    ));
    assert!(
        error.contains(
            "views.ship.release.blocker_labels: a release names at most 50 blocker labels"
        ),
        "{error}"
    );
    let error = errors(
        "{ milestone = \"[\", label = \"[\", branch = \"[\", pull = { head = \"[\" }, blocker_labels = [\"[\"] }",
    );
    for key in [
        "views.ship.release.milestone:",
        "views.ship.release.label:",
        "views.ship.release.branch:",
        "views.ship.release.pull.head:",
        "views.ship.release.blocker_labels[0]:",
    ] {
        assert!(error.contains(key), "{key}\n{error}");
    }
    let error = errors("{ pull = { title = \"[\" } }");
    assert!(error.contains("views.ship.release.pull.title:"), "{error}");
    let error = errors("{ milestone = \"v2\", blocked_by = [\"base\"] }");
    assert!(error.contains("views.ship"), "{error}");
}
