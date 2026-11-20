//! A package, owner or tag whose name is a dot segment, never asked for.

use super::*;

#[test]
fn a_package_owner_or_tag_named_by_a_dot_segment_is_never_asked_for() {
    let chosen = || SourceOptions {
        package_types: Some(vec!["container".to_owned()]),
        ..SourceOptions::default()
    };
    let dotted = ScriptedHttp::new(vec![
        github_repository("User"),
        ok(&json!([github_package("..", Some(7))])),
    ]);
    let refused = polled(&releases(Forge::GitHub, chosen()), &dotted, None);
    assert!(
        matches!(&refused, Err(Unpolled::Unreadable(reason)) if reason.contains("dot segment")),
        "{refused:?}"
    );
    assert_eq!(urls(&dotted).len(), 2);
    let owner = ScriptedHttp::new(vec![ok(&json!({
        "id": 7, "name": "r", "html_url": "https://github.com/O/r", "visibility": "public",
        "owner": {"login": "..", "type": "User"}
    }))]);
    let refused = polled(&releases(Forge::GitHub, chosen()), &owner, None);
    assert!(
        matches!(refused, Err(Unpolled::Unreadable(_))),
        "{refused:?}"
    );
    assert_eq!(urls(&owner).len(), 1);
    let tagged = ScriptedHttp::new(vec![
        gitlab_project(),
        ok(&json!([{"id": 3, "name": ""}])),
        ok(&json!([{"name": "..", "location": "registry.gitlab.example/g/s/r:.."}])),
    ]);
    let images = SourceOptions {
        packages: vec!["g/s/r".to_owned()],
        ..chosen()
    };
    let refused = polled(
        &releases(Forge::GitLab, images),
        &tagged,
        Some(&polled_packages()),
    );
    assert!(
        matches!(refused, Err(Unpolled::Unreadable(_))),
        "{refused:?}"
    );
    assert_eq!(urls(&tagged).len(), 3);
}
