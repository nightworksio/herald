use jiff::Timestamp;

use super::*;

fn running(
    repository: &str,
    visibility: Visibility,
    (sha, pull, queued): (&str, bool, bool),
) -> Running {
    Running {
        repository: repository.to_owned(),
        visibility,
        id: sha.to_owned(),
        url: format!("https://github.com/{repository}/actions/runs/{sha}"),
        sha: sha.to_owned(),
        pull,
        queued,
        created: Timestamp::from_second(1_000).unwrap(),
    }
}

fn overviews() -> Overviews {
    Overviews {
        capacity: Some(vec![
            running("o/app", Visibility::Public, ("h1", true, true)),
            running("o/app", Visibility::Public, ("old", true, false)),
            running("o/app", Visibility::Public, ("main", false, false)),
            running("o/vault", Visibility::Private, ("gone", true, true)),
        ]),
        unread: vec![Unread {
            repository: "o/vault".to_owned(),
            visibility: Visibility::Private,
            reason: "the token may not read its runs".to_owned(),
        }],
        heads: vec!["h1".to_owned()],
    }
}

#[test]
fn a_run_for_a_pull_request_on_no_open_head_is_superseded() {
    let overviews = overviews();
    let ids: Vec<&str> = overviews
        .superseded()
        .into_iter()
        .map(|run| run.id.as_str())
        .collect();
    assert_eq!(ids, ["old", "gone"]);
}

#[test]
fn an_audience_sees_only_the_runs_and_unread_repositories_it_may_see() {
    let public = overviews().seen_by(Visibility::Public);
    assert_eq!(public.capacity.as_ref().map(Vec::len), Some(3));
    assert_eq!(public.unread, []);
    assert_eq!(overviews().seen_by(Visibility::Private), overviews());
    assert_eq!(
        Overviews::default().seen_by(Visibility::Public),
        Overviews::default()
    );
}

#[test]
fn a_view_shows_only_the_runs_and_unread_repositories_of_the_repositories_it_holds() {
    let view = View {
        repositories: vec![crate::Glob::new("o/app*").unwrap()],
        ..View::default()
    };
    let repository = |name: &str| name.contains('/');
    let within = overviews().within(&view, repository);
    assert!(
        within
            .capacity
            .iter()
            .flatten()
            .all(|run| run.repository == "o/app")
    );
    assert_eq!(within.capacity.as_ref().map(Vec::len), Some(3));
    assert_eq!(within.unread, []);
    assert_eq!(
        overviews().within(&View::default(), repository),
        overviews()
    );
    let failed = Overviews {
        unread: vec![Unread {
            repository: "org".to_owned(),
            visibility: Visibility::Private,
            reason: "its runs could not be read".to_owned(),
        }],
        ..overviews()
    };
    assert_eq!(failed.within(&view, repository).unread.len(), 1);
}

#[test]
fn merged_overviews_hold_the_runs_and_unread_repositories_of_both() {
    let mut merged = Overviews::default();
    merged.merge(Overviews {
        capacity: None,
        ..overviews()
    });
    assert_eq!((merged.capacity.as_ref(), merged.unread.len()), (None, 1));
    merged.merge(overviews());
    merged.merge(overviews());
    assert_eq!(
        (merged.capacity.map(|runs| runs.len()), merged.unread.len()),
        (Some(8), 3)
    );
}
