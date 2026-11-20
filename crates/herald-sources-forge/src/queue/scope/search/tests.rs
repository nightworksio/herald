use serde_json::json;

use super::*;

#[test]
fn a_repository_s_path_is_the_last_two_segments_of_its_api_address() {
    assert_eq!(
        path_of("https://ghe.example/api/v3/repos/o/app/"),
        Some("o/app".to_owned())
    );
    assert_eq!(path_of("app"), None);
    assert_eq!(path_of("/app"), None);
}

#[test]
fn a_github_item_whose_repository_cannot_be_told_is_left_out_and_still_counted() {
    let body = json!({ "total_count": 2, "items": [
        { "number": 1, "title": "t", "html_url": "https://github.com/o/app/pull/1",
          "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
          "repository_url": "app", "draft": true },
        { "number": 2, "title": "t", "html_url": "https://github.com/o/app/pull/2",
          "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
          "repository_url": "https://api.github.com/repos/o/app" }
    ] });
    let (pulls, items, total) = github(body.to_string().as_bytes(), 1 << 20).unwrap();
    assert_eq!((items, total), (2, Some(2)));
    let numbers: Vec<(Located, u64, bool)> = pulls
        .into_iter()
        .map(|(located, pull)| (located, pull.number, pull.bot))
        .collect();
    assert_eq!(numbers, [(Located::Path("o/app".to_owned()), 2, false)]);
}

#[test]
fn a_forgejo_item_that_is_no_pull_request_or_names_no_repository_is_left_out() {
    let body = json!([
        { "number": 1, "title": "t", "html_url": "https://codeberg.org/o/app/issues/1",
          "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
          "repository": { "id": 3 } },
        { "number": 2, "title": "t", "html_url": "https://codeberg.org/o/app/pulls/2",
          "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
          "pull_request": {} },
        { "number": 3, "title": "t", "html_url": "https://codeberg.org/o/app/pulls/3",
          "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
          "repository": { "id": 3 }, "pull_request": {}, "user": { "login": "ci-bot" },
          "labels": [{ "name": "deps" }] }
    ]);
    let (pulls, items, total) = forgejo(body.to_string().as_bytes(), 1 << 20).unwrap();
    assert_eq!((items, total), (3, None));
    let (located, pull) = pulls.first().unwrap();
    assert_eq!(*located, Located::Id("3".to_owned()));
    assert_eq!(
        (
            pull.number,
            pull.draft,
            pull.labels.clone(),
            pull.author.as_deref()
        ),
        (3, false, vec!["deps".to_owned()], Some("ci-bot"))
    );
    assert_eq!(pulls.len(), 1);
}

#[test]
fn an_answer_that_is_no_search_does_not_read() {
    assert!(github(b"[]", 1 << 20).is_err());
    assert!(forgejo(b"{}", 1 << 20).is_err());
}
