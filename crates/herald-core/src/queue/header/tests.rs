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

fn release(withheld: crate::queue::Withheld, repository: Option<&str>) -> crate::queue::Release {
    crate::queue::Release {
        milestone: Some(Glob::new("v2.*").unwrap()),
        label: None,
        branch: None,
        pull: None,
        combine: crate::queue::Combine::All,
        blocker_labels: vec![Glob::new("blocker").unwrap()],
        blocked_by: crate::queue::Blocker::ALL.to_vec(),
        withheld,
        header: true,
        rows: crate::queue::Rows::Pulls,
        repository: repository.map(str::to_owned),
    }
}

fn released(repositories: &[&str], repository: Option<&str>) -> View {
    View {
        repositories: repositories
            .iter()
            .map(|glob| Glob::new(glob).unwrap())
            .collect(),
        release: Some(release(crate::queue::Withheld::Unknown, repository)),
        ..View::default()
    }
}

fn member(path: &str, visibility: Visibility) -> Listed {
    Listed {
        id: path.to_owned(),
        name: path.rsplit('/').next().unwrap().to_owned(),
        path: path.to_owned(),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    }
}

fn carried(repository: &str, visibility: Visibility, milestone: &str) -> Pull {
    Pull {
        repository: repository.to_owned(),
        visibility,
        milestone: Some(milestone.to_owned()),
        ..crate::queue::tests::pull()
    }
}

fn headed(path: &str, visibility: Visibility, (picked, group): (bool, bool)) -> Headed {
    Headed {
        source: 0,
        path: path.to_owned(),
        visibility,
        picked,
        group,
    }
}

fn read(headed: &[Headed]) -> Vec<(&str, Visibility, bool, bool)> {
    headed
        .iter()
        .map(|headed| {
            (
                headed.path.as_str(),
                headed.visibility,
                headed.picked,
                headed.group,
            )
        })
        .collect()
}

#[test]
fn a_source_asks_for_its_picked_repositories_its_own_its_members_and_their_groups_under_its_owner()
{
    let view = released(&["o/api", "o/sub/web", "p/q/r"], None);
    let pulls = [
        carried("o/api", Visibility::Public, "v2.0"),
        carried("o/sub/web", Visibility::Internal, "v9.0"),
    ];
    let members = [
        member("o/api", Visibility::Public),
        member("o/sub/web", Visibility::Internal),
        member("o/sub/deep/x", Visibility::Public),
        member("o/subway", Visibility::Private),
        member("o/secret", Visibility::Private),
        member("p/q/r", Visibility::Public),
    ];
    let asked = header_asked(&view, (3, Some("O")), (&pulls, Some("o/own"), &members));
    assert!(asked.iter().all(|headed| headed.source == 3));
    assert_eq!(
        read(&asked),
        [
            ("o/api", Visibility::Public, true, false),
            ("o/own", Visibility::Private, false, false),
            ("o/api", Visibility::Public, false, false),
            ("o/sub/web", Visibility::Internal, false, false),
            ("o/sub/deep/x", Visibility::Public, false, false),
            ("o/subway", Visibility::Private, false, false),
            ("o/secret", Visibility::Private, false, false),
            ("p/q/r", Visibility::Public, false, false),
            ("o", Visibility::Private, false, true),
            ("o/sub", Visibility::Internal, false, true),
        ]
    );
    let ungrouped = header_asked(&view, (0, None), (&pulls, None, &members));
    assert!(ungrouped.iter().all(|headed| !headed.group));
    let mut unheaded = released(&[], None);
    if let Some(release) = unheaded.release.as_mut() {
        release.header = false;
    }
    assert_eq!(
        header_asked(&unheaded, (0, None), (&pulls, Some("o/own"), &[])),
        []
    );
    let unreleased = View::default();
    assert_eq!(
        header_asked(&unreleased, (0, None), (&pulls, None, &[])),
        []
    );
}

#[test]
fn a_header_reads_each_repository_once_held_public_first_then_picked_then_by_path() {
    let view = released(&["o/*"], None);
    let asked = vec![
        headed("o/zed", Visibility::Private, (true, false)),
        headed("o/own", Visibility::Private, (false, false)),
        headed("O/Own", Visibility::Public, (true, false)),
        headed("o/beta", Visibility::Internal, (false, false)),
        headed("o/alpha", Visibility::Internal, (true, false)),
        headed("o/web", Visibility::Public, (false, false)),
        headed("x/y", Visibility::Public, (true, false)),
        headed("o", Visibility::Internal, (false, true)),
        headed("o/web", Visibility::Public, (false, false)),
    ];
    let (read_, unread) = heading(&view, asked);
    assert_eq!(
        read(&read_),
        [
            ("o/own", Visibility::Public, true, false),
            ("o/web", Visibility::Public, false, false),
            ("o/alpha", Visibility::Internal, true, false),
            ("o", Visibility::Internal, false, true),
            ("o/beta", Visibility::Internal, false, false),
            ("o/zed", Visibility::Private, true, false),
        ]
    );
    assert_eq!(unread, []);
}

#[test]
fn a_release_held_to_one_repository_reads_it_alone_and_no_group() {
    let view = released(&[], Some("O/App"));
    let asked = vec![
        headed("o/app", Visibility::Public, (false, false)),
        headed("o/web", Visibility::Public, (true, false)),
        headed("o", Visibility::Public, (false, true)),
    ];
    let (read_, _) = heading(&view, asked);
    assert_eq!(read(&read_), [("o/app", Visibility::Public, false, false)]);
    let release = view.release.as_ref().unwrap();
    assert!(release.takes("o/APP") && !release.takes("o/web"));
    assert!(!release.picks(&carried("o/web", Visibility::Public, "v2.0")));
    assert!(release.picks(&carried("o/app", Visibility::Public, "v2.0")));
    let spanning = released(&[], None);
    assert!(spanning.release.as_ref().unwrap().takes("o/web"));
    let (read_, _) = heading(
        &View::default(),
        vec![headed("o", Visibility::Public, (false, true))],
    );
    assert_eq!(read(&read_), [("o", Visibility::Public, false, true)]);
}

#[test]
fn a_header_reads_at_most_the_most_and_names_the_rest_unread_as_each_is_seen() {
    let view = released(&[], None);
    let mut asked: Vec<Headed> = (0..MOST_HEADER_REPOSITORIES)
        .map(|index| {
            headed(
                &format!("o/p{index:02}"),
                Visibility::Public,
                (false, false),
            )
        })
        .collect();
    asked.push(headed("o/vault", Visibility::Private, (true, false)));
    asked.push(headed("o/inside", Visibility::Internal, (false, false)));
    let (read_, unread) = heading(&view, asked.clone());
    assert_eq!(read_.len(), MOST_HEADER_REPOSITORIES);
    assert!(
        read_
            .iter()
            .all(|headed| headed.visibility == Visibility::Public)
    );
    let named: Vec<(&str, Visibility)> = unread
        .iter()
        .map(|unread| (unread.repository.as_str(), unread.visibility))
        .collect();
    assert_eq!(
        named,
        [
            ("o/inside", Visibility::Internal),
            ("o/vault", Visibility::Private)
        ]
    );
    assert_eq!(
        unread[0].reason,
        "a release's header reads the milestones of at most 20 repositories and groups"
    );
    asked.truncate(MOST_HEADER_REPOSITORIES);
    let (read_, unread) = heading(&view, asked.clone());
    assert_eq!((read_.len(), unread.len()), (MOST_HEADER_REPOSITORIES, 0));
    asked.truncate(MOST_HEADER_REPOSITORIES - 1);
    let (read_, unread) = heading(&view, asked);
    assert_eq!(
        (read_.len(), unread.len()),
        (MOST_HEADER_REPOSITORIES - 1, 0)
    );
}

#[test]
fn a_group_s_issues_are_each_seen_as_the_member_holding_it_and_kept_where_the_view_holds_it() {
    let view = released(&["o/api", "o/web", "o/gone"], None);
    let members = [
        member("o/api", Visibility::Public),
        member("o/web", Visibility::Internal),
    ];
    let found = Header {
        milestones: vec![milestone("o", Visibility::Internal)],
        issues: Some(vec![
            issue("o/api", Visibility::Public),
            issue("o/api", Visibility::Private),
            issue("O/WEB", Visibility::Public),
            issue("o/gone", Visibility::Public),
            issue("o/hidden", Visibility::Public),
        ]),
        unread: Vec::new(),
    };
    let group = headed("o", Visibility::Internal, (false, true));
    let seen = found.clone().read_as(&view, (&group, &members));
    let issues: Vec<(&str, Visibility)> = seen
        .issues
        .iter()
        .flatten()
        .map(|issue| (issue.repository.as_str(), issue.visibility))
        .collect();
    assert_eq!(
        issues,
        [
            ("o/api", Visibility::Public),
            ("o/api", Visibility::Private),
            ("O/WEB", Visibility::Internal),
            ("o/gone", Visibility::Private),
        ]
    );
    assert_eq!(seen.milestones, found.milestones);
    let repository = headed("o/api", Visibility::Public, (false, false));
    assert_eq!(found.clone().read_as(&view, (&repository, &[])), found);
    let unlisted = Header {
        issues: None,
        ..found
    };
    assert_eq!(unlisted.read_as(&view, (&group, &members)).issues, None);
}

#[test]
fn a_public_audience_learns_nothing_of_a_private_or_internal_member_under_each_withheld_mode() {
    use crate::queue::{Beside, Withheld, grouped, rows};
    let now = jiff::Timestamp::from_second(1_000_000).unwrap();
    let mut members: Vec<Listed> = (0..MOST_HEADER_REPOSITORIES)
        .map(|index| member(&format!("o/p{index:02}"), Visibility::Public))
        .collect();
    members.push(member("o/vault", Visibility::Private));
    members.push(member("o/inside", Visibility::Internal));
    let public = [carried("o/p00", Visibility::Public, "v2.0")];
    let private = [Pull {
        labels: vec!["blocker".to_owned()],
        ..carried("o/vault", Visibility::Private, "v2.0-vault")
    }];
    for withheld in [Withheld::Unknown, Withheld::Shown, Withheld::Masked] {
        let view = View {
            release: Some(release(withheld, None)),
            ..View::default()
        };
        let pulls: Vec<Pull> = public.iter().chain(&private).cloned().collect();
        let asked = header_asked(&view, (0, Some("o")), (&pulls, None, &members));
        let (read_, bounded) = heading(&view, asked);
        let mut header = Header {
            unread: bounded,
            issues: Some(Vec::new()),
            ..Header::default()
        };
        for headed in &read_ {
            let found = Header {
                milestones: vec![OpenMilestone {
                    title: format!("v2.0-{}", headed.path),
                    counts: (headed.visibility != Visibility::Public).then_some((7_301, 7_302)),
                    ..milestone(&headed.path, headed.visibility)
                }],
                issues: Some(vec![issue(&headed.path, headed.visibility)]),
                unread: vec![unread(&headed.path, headed.visibility)],
            };
            header.merge(found.read_as(&view, (headed, &members)));
        }
        let mut private_view = header.clone();
        private_view.merge(Header {
            milestones: vec![OpenMilestone {
                counts: Some((7_301, 7_302)),
                ..milestone("o/vault", Visibility::Private)
            }],
            issues: Some(vec![issue("o/inside", Visibility::Internal)]),
            unread: vec![unread("o/inside", Visibility::Internal)],
        });
        let overviews = Overviews {
            header: Some(private_view),
            ..Overviews::default()
        };
        let seen = overviews.seen_by(Visibility::Public);
        let hidden = rows(&view, &private, now);
        let beside = Beside {
            withheld: &hidden,
            ..Beside::default()
        };
        let board = grouped(&view, rows(&view, &public, now), beside);
        let told = format!("{seen:?} {:?} {:?}", board.readiness, board.groups);
        for secret in ["vault", "inside", "7301", "7302", "v2.0-vault"] {
            assert!(!told.contains(secret), "{withheld:?} told {secret}: {told}");
        }
        assert!(told.contains("o/p00"), "{told}");
        assert_eq!(seen.header.unwrap().strictest(), Some(Visibility::Public));
    }
}
