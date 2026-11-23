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

fn head(repository: &str, sha: &str) -> Head {
    Head {
        repository: repository.to_owned(),
        sha: sha.to_owned(),
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
        heads: vec![head("o/app", "h1")],
        advisories: None,
        load: None,
        header: None,
        bases: vec![
            based("o/app", Visibility::Public),
            based("o/vault", Visibility::Private),
        ],
    }
}

fn based(repository: &str, visibility: Visibility) -> crate::queue::Based {
    crate::queue::Based {
        repository: repository.to_owned(),
        visibility,
        branch: "main".to_owned(),
        read: crate::queue::BaseRead::Checked(crate::queue::Checks::Passed),
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
fn only_a_head_in_a_run_s_own_repository_keeps_it_from_being_superseded() {
    let mut overviews = overviews();
    overviews.heads = vec![head("o/mirror", "h1"), head("O/App", "old")];
    let ids: Vec<&str> = overviews
        .superseded()
        .into_iter()
        .map(|run| run.id.as_str())
        .collect();
    assert_eq!(ids, ["h1", "gone"]);
}

#[test]
fn an_audience_sees_only_the_runs_and_unread_repositories_it_may_see() {
    let public = overviews().seen_by(Visibility::Public);
    assert_eq!(public.capacity.as_ref().map(Vec::len), Some(3));
    assert_eq!(public.unread, []);
    assert_eq!(public.bases, [based("o/app", Visibility::Public)]);
    assert_eq!(
        overviews().seen_by(Visibility::Internal).bases,
        [based("o/app", Visibility::Public)]
    );
    assert_eq!(overviews().seen_by(Visibility::Private), overviews());
    let counted = Overviews {
        load: Some(crate::queue::Loads::default()),
        ..overviews()
    };
    assert_eq!(counted.seen_by(Visibility::Private).load, None);
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
    assert_eq!(within.bases, [based("o/app", Visibility::Public)]);
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
    assert_eq!(merged.bases.len(), 6);
}
