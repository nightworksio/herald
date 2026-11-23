use super::*;
use crate::Glob;
use crate::queue::{Overviews, Printed};

fn milestone(repository: &str, visibility: Visibility) -> OpenMilestone {
    OpenMilestone {
        repository: repository.to_owned(),
        visibility,
        title: "v2.0".to_owned(),
        url: None,
        due: None,
        overdue: false,
        counts: None,
    }
}

fn unread(repository: &str, visibility: Visibility) -> Unread {
    Unread {
        repository: repository.to_owned(),
        visibility,
        reason: "refused".to_owned(),
    }
}

fn issue(repository: &str, visibility: Visibility) -> OpenIssue {
    OpenIssue {
        repository: repository.to_owned(),
        visibility,
        number: 7,
        title: "Crash".to_owned(),
        url: format!("https://github.com/{repository}/issues/7"),
        milestone: "v2.0".to_owned(),
    }
}

fn issued(header: &Header) -> Vec<&str> {
    header
        .issues
        .iter()
        .flatten()
        .map(|issue| issue.repository.as_str())
        .collect()
}

fn header() -> Header {
    Header {
        milestones: vec![
            milestone("o/app", Visibility::Public),
            milestone("o/inner", Visibility::Internal),
            milestone("o/vault", Visibility::Private),
        ],
        issues: Some(vec![
            issue("o/app", Visibility::Public),
            issue("o/inner", Visibility::Internal),
            issue("o/vault", Visibility::Private),
            issue("o/app", Visibility::Private),
        ]),
        unread: vec![
            unread("o/app", Visibility::Public),
            unread("o/inner", Visibility::Internal),
            unread("o/vault", Visibility::Private),
        ],
    }
}

fn repositories(header: &Header) -> (Vec<&str>, Vec<&str>) {
    (
        header
            .milestones
            .iter()
            .map(|milestone| milestone.repository.as_str())
            .collect(),
        header
            .unread
            .iter()
            .map(|unread| unread.repository.as_str())
            .collect(),
    )
}

#[test]
fn an_audience_is_shown_the_milestones_and_unread_repositories_it_may_see_alone() {
    let overviews = Overviews {
        header: Some(header()),
        ..Overviews::default()
    };
    let public = overviews.seen_by(Visibility::Public).header.unwrap();
    assert_eq!(repositories(&public), (vec!["o/app"], vec!["o/app"]));
    assert_eq!(issued(&public), ["o/app"]);
    assert_eq!(public.issues.unwrap()[0].visibility, Visibility::Public);
    let internal = overviews.seen_by(Visibility::Internal).header.unwrap();
    assert_eq!(
        repositories(&internal),
        (vec!["o/app", "o/inner"], vec!["o/app", "o/inner"])
    );
    assert_eq!(issued(&internal), ["o/app", "o/inner"]);
    let private = overviews.seen_by(Visibility::Private).header.unwrap();
    assert_eq!(private, header());
    assert_eq!(
        Overviews::default().seen_by(Visibility::Private).header,
        None
    );
}

#[test]
fn herald_queue_prints_the_header_of_the_repositories_it_prints_alone() {
    let overviews = Overviews {
        header: Some(header()),
        ..Overviews::default()
    };
    let withheld = Printed::Withheld.overviews(overviews.clone(), &[]);
    let withheld = withheld.header.unwrap();
    assert_eq!(repositories(&withheld), (vec!["o/app"], vec!["o/app"]));
    assert_eq!(issued(&withheld), ["o/app"]);
    let asked = Printed::Asked.overviews(overviews, &[]);
    assert_eq!(asked.header.unwrap(), header());
    assert_eq!(
        Printed::Asked.overviews(Overviews::default(), &[]).header,
        None
    );
}

#[test]
fn a_view_is_shown_the_header_of_the_repositories_it_holds_alone() {
    let view = View {
        repositories: vec![Glob::new("o/app").unwrap()],
        ..View::default()
    };
    let overviews = Overviews {
        header: Some(header()),
        ..Overviews::default()
    };
    let within = overviews.within(&view, |_| true).header.unwrap();
    assert_eq!(repositories(&within), (vec!["o/app"], vec!["o/app"]));
    assert_eq!(issued(&within), ["o/app", "o/app"]);
    let unlisted = Header {
        issues: None,
        ..header()
    };
    assert_eq!(unlisted.within(&view).issues, None);
}

#[test]
fn a_merged_header_holds_both_and_one_is_made_where_none_was() {
    let mut overviews = Overviews::default();
    overviews.merge(Overviews::default());
    assert_eq!(overviews.header, None);
    overviews.merge(Overviews {
        header: Some(header()),
        ..Overviews::default()
    });
    overviews.merge(Overviews {
        header: Some(header()),
        ..Overviews::default()
    });
    let merged = overviews.header.unwrap();
    assert_eq!(merged.milestones.len(), 6);
    assert_eq!(merged.issues.as_ref().map(Vec::len), Some(8));
    assert_eq!(merged.unread.len(), 6);
    let mut unlisted = Header::default();
    unlisted.merge(Header::default());
    assert_eq!(unlisted.issues, None);
    unlisted.merge(Header {
        issues: Some(Vec::new()),
        ..Header::default()
    });
    assert_eq!(unlisted.issues, Some(Vec::new()));
}

#[test]
fn a_header_s_strictest_visibility_is_of_its_milestones_and_unread_repositories() {
    assert_eq!(Header::default().strictest(), None);
    assert_eq!(header().strictest(), Some(Visibility::Private));
    let unread_only = Header {
        milestones: vec![milestone("o/app", Visibility::Public)],
        issues: None,
        unread: vec![unread("o/inner", Visibility::Internal)],
    };
    assert_eq!(unread_only.strictest(), Some(Visibility::Internal));
    let milestones_only = Header {
        milestones: vec![milestone("o/inner", Visibility::Internal)],
        issues: Some(vec![issue("o/app", Visibility::Public)]),
        unread: vec![unread("o/app", Visibility::Public)],
    };
    assert_eq!(milestones_only.strictest(), Some(Visibility::Internal));
    let issues_only = Header {
        milestones: vec![milestone("o/app", Visibility::Public)],
        issues: Some(vec![issue("o/app", Visibility::Private)]),
        unread: vec![unread("o/app", Visibility::Public)],
    };
    assert_eq!(issues_only.strictest(), Some(Visibility::Private));
}
