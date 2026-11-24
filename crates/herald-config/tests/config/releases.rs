//! A view's `release`: the selectors that pick its pull requests, how they
//! combine, and what holds it back, every mistake named by key
//! (ADR-0081).

use herald_config::{Config, load};
use herald_core::{
    BLOCKER_LABELS, Blocker, Combine, Glob, HeaderRead, MOST_BLOCKER_LABELS, Release, ReleasePull,
    Rows, Withheld,
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
            withheld: Withheld::Unknown,
            header: true,
            rows: herald_core::Rows::Pulls,
            repository: None,
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
            withheld: Withheld::Unknown,
            header: true,
            rows: herald_core::Rows::Pulls,
            repository: None,
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
        loaded("{ label = \"ship\" }").label,
        Some(Glob::new("ship").unwrap())
    );
    assert_eq!(
        loaded("{ branch = \"release/*\" }").branch,
        Some(Glob::new("release/*").unwrap())
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
    let most: Vec<String> = (0..MOST_BLOCKER_LABELS)
        .map(|n| format!("\"l{n}\""))
        .collect();
    let release = loaded(&format!(
        "{{ milestone = \"v2\", blocker_labels = [{}] }}",
        most.join(", ")
    ));
    assert_eq!(release.blocker_labels.len(), MOST_BLOCKER_LABELS);
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
    let error = errors("{ milestone = \"v2\", blocked_by = [\"merge\"] }");
    assert!(error.contains("views.ship"), "{error}");
    assert_eq!(
        loaded("{ milestone = \"v2\", blocked_by = [\"base\"] }").blocked_by,
        [herald_core::Blocker::Base]
    );
    let error = errors("{ milestone = \"v2\", withheld = \"hidden\" }");
    assert!(error.contains("views.ship"), "{error}");
}

#[test]
fn how_a_release_is_told_where_pull_requests_are_withheld_is_read() {
    let told = |withheld: &str| {
        loaded(&format!(
            "{{ milestone = \"v2\", withheld = \"{withheld}\" }}"
        ))
        .withheld
    };
    assert_eq!(
        ["unknown", "shown", "masked"].map(told),
        [Withheld::Unknown, Withheld::Shown, Withheld::Masked]
    );
}

#[test]
fn a_release_is_headed_by_its_milestones_unless_it_says_not_and_a_header_needs_a_milestone() {
    let glob = Glob::new("v2").unwrap();
    let heading = |header: bool, issues: bool| Some((&glob, HeaderRead { header, issues }));
    let headed = loaded("{ milestone = \"v2\" }");
    assert!(headed.header);
    assert_eq!(headed.heading(), heading(true, false));
    let unheaded = loaded("{ milestone = \"v2\", header = false }");
    assert!(!unheaded.header);
    assert_eq!(unheaded.heading(), None);
    let labelled = loaded("{ label = \"ship\" }");
    assert!(labelled.header);
    assert_eq!(labelled.heading(), None);
    assert!(!loaded("{ label = \"ship\", header = false }").header);
    let error = errors("{ label = \"ship\", header = true }");
    assert!(
        error.contains("views.ship.release.header: a release's header names the open milestones its milestone selector matches"),
        "{error}"
    );
}

#[test]
fn a_release_lists_pull_requests_unless_it_lists_issues_and_issue_rows_need_a_milestone() {
    let glob = Glob::new("v2").unwrap();
    let heading = |header: bool, issues: bool| Some((&glob, HeaderRead { header, issues }));
    assert_eq!(loaded("{ milestone = \"v2\" }").rows, Rows::Pulls);
    assert_eq!(
        loaded("{ milestone = \"v2\", rows = \"pulls\" }").rows,
        Rows::Pulls
    );
    let listed = loaded("{ milestone = \"v2\", rows = \"issues\" }");
    assert_eq!(listed.rows, Rows::Issues);
    assert_eq!(listed.heading(), heading(true, true));
    let unheaded = loaded("{ milestone = \"v2\", rows = \"issues\", header = false }");
    assert_eq!(unheaded.heading(), heading(false, true));
    assert_eq!(
        loaded("{ label = \"ship\", rows = \"pulls\" }").rows,
        Rows::Pulls
    );
    let error = errors("{ label = \"ship\", rows = \"issues\" }");
    assert!(
        error.contains("views.ship.release.rows: a release's issue rows are the open issues of the open milestones its milestone selector matches"),
        "{error}"
    );
    assert!(errors("{ milestone = \"v2\", rows = \"people\" }").contains("rows"));
}

#[test]
fn a_release_is_held_to_one_repository_the_view_holds_and_spans_them_all_by_default() {
    assert_eq!(loaded("{ milestone = \"v2\" }").repository, None);
    let held = loaded("{ milestone = \"v2\", repository = \"nightworksio/herald\" }");
    assert_eq!(held.repository.as_deref(), Some("nightworksio/herald"));
    let within = |release: &str| {
        let text = format!(
            "{EXAMPLE}\n[views.ship]\nrepositories = [\"nightworksio/*\"]\nrelease = {release}\n"
        );
        load("herald.toml", &text, &nothing, &crate::common::kinds()).map(|_| ())
    };
    assert!(within("{ milestone = \"v2\", repository = \"NightWorksIO/herald\" }").is_ok());
    let error = within("{ milestone = \"v2\", repository = \"other/app\" }")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(
            "views.ship.release.repository: other/app is not a repository the view holds"
        ),
        "{error}"
    );
}
