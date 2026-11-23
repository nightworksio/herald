use jiff::Timestamp;

use super::*;
use crate::members::Visibility;

fn listed(surface: Surface) -> usize {
    match surface {
        Surface::Route {
            private: true,
            recallable: true,
        } => 0,
        Surface::Route {
            private: true,
            recallable: false,
        } => 1,
        Surface::Route {
            private: false,
            recallable: true,
        } => 2,
        Surface::Route {
            private: false,
            recallable: false,
        } => 3,
        Surface::Queue {
            private: Printed::Asked,
        } => 4,
        Surface::Queue {
            private: Printed::Configured,
        } => 5,
        Surface::Queue {
            private: Printed::Withheld,
        } => 6,
        Surface::Digest => 7,
        Surface::Feed => 8,
        Surface::Push => 9,
        Surface::Rule => 10,
        Surface::Told => 11,
        Surface::Doctor => 12,
        Surface::Api => 13,
    }
}

fn found() -> Advisories {
    Advisories {
        open: vec![Advisory {
            repository: "o/app".to_owned(),
            tool: "secret-scanning".to_owned(),
            severity: None,
            summary: Some("Example Service Key".to_owned()),
            state: "open".to_owned(),
            url: "https://github.com/o/app/security/secret-scanning/1".to_owned(),
            opened: Timestamp::from_second(1_000).unwrap(),
        }],
        unread: vec![Unread {
            repository: "o/vault".to_owned(),
            visibility: Visibility::Private,
            reason: "refused".to_owned(),
        }],
        more: true,
        at: Timestamp::from_second(2_000).unwrap(),
    }
}

#[test]
fn every_surface_is_listed_once_and_decided() {
    let order: Vec<usize> = Surface::ALL.into_iter().map(listed).collect();
    assert_eq!(order, (0..Surface::ALL.len()).collect::<Vec<_>>());
    let advised: Vec<Surface> = Surface::ALL.into_iter().filter(|s| advises(*s)).collect();
    assert_eq!(
        advised,
        [
            Surface::Route {
                private: true,
                recallable: true,
            },
            Surface::Queue {
                private: Printed::Asked
            }
        ]
    );
}

#[test]
fn a_forge_s_teams_reach_a_private_route_and_a_queue_said_private_alone() {
    let reached: Vec<Surface> = Surface::ALL
        .into_iter()
        .filter(|s| teams_reach(*s))
        .collect();
    assert_eq!(
        reached,
        [
            Surface::Route {
                private: true,
                recallable: true,
            },
            Surface::Route {
                private: true,
                recallable: false,
            },
            Surface::Queue {
                private: Printed::Asked
            },
            Surface::Queue {
                private: Printed::Configured
            },
        ]
    );
}

#[test]
fn a_forge_s_teams_reach_each_surface_only_where_there_is_one_and_every_one_is_reached() {
    let private = Surface::Route {
        private: true,
        recallable: false,
    };
    let public = Surface::Route {
        private: false,
        recallable: false,
    };
    assert!(!teams_reach_each([]));
    assert!(teams_reach_each([private]));
    assert!(teams_reach_each([private, private]));
    assert!(!teams_reach_each([public]));
    assert!(!teams_reach_each([private, public]));
    assert!(!teams_reach_each([public, private]));
}

#[test]
fn a_section_is_shown_to_a_private_surface_alone_and_withheld_whole_elsewhere() {
    let found = found();
    for surface in Surface::ALL {
        let advised = Advised::to(&found, surface);
        if advises(surface) {
            assert_eq!(advised.shown(), Some(&found), "{surface:?}");
        } else {
            assert_eq!(advised.shown(), None, "{surface:?}");
            assert_eq!(
                advised,
                Advised::to(&Advisories::none(found.at), surface),
                "{surface:?}"
            );
        }
    }
}

#[test]
fn merging_adds_every_advisory_unread_repository_and_overflow() {
    let mut all = Advisories::none(Timestamp::from_second(2_000).unwrap());
    all.merge(found());
    assert_eq!(all.open.len(), 1);
    assert_eq!(all.unread.len(), 1);
    assert!(all.more);
    all.merge(Advisories::none(all.at));
    assert!(all.more);
    assert_eq!(all.open.len(), 1);
    all.merge(found());
    assert!(all.more);
}

#[test]
fn the_withheld_line_names_no_section() {
    assert!(!WITHHELD.to_lowercase().contains("advis"));
    assert!(!WITHHELD.to_lowercase().contains("security"));
}

fn advisory_of(repository: &str) -> Advisory {
    Advisory {
        repository: repository.to_owned(),
        ..found().open[0].clone()
    }
}

fn unread_of(repository: &str) -> Unread {
    Unread {
        repository: repository.to_owned(),
        visibility: Visibility::Private,
        reason: "refused".to_owned(),
    }
}

#[test]
fn a_view_shows_only_the_advisories_and_unread_repositories_it_holds() {
    let view = View {
        repositories: vec![crate::Glob::new("o/pay*").unwrap()],
        ..View::default()
    };
    let read = Advisories {
        open: vec![advisory_of("o/pay-api"), advisory_of("o/other")],
        unread: vec![unread_of("o/pay-web"), unread_of("o/other"), unread_of("o")],
        ..Advisories::none(found().at)
    };
    let within = read.clone().within(&view, |path| path.contains('/'));
    let open: Vec<&str> = within
        .open
        .iter()
        .map(|row| row.repository.as_str())
        .collect();
    assert_eq!(open, ["o/pay-api"]);
    let unread: Vec<&str> = within
        .unread
        .iter()
        .map(|row| row.repository.as_str())
        .collect();
    assert_eq!(unread, ["o/pay-web", "o"]);
    assert_eq!(read.clone().within(&View::default(), |_| true), read);
}

#[test]
fn a_sample_section_holds_made_up_rows_and_a_withheld_one_none() {
    let at = found().at;
    let sample = Advised::sample(at);
    let shown = sample.shown().unwrap();
    assert_eq!(shown.open.len(), 1);
    assert!(shown.open[0].repository.starts_with("sample-owner/"));
    assert_eq!(Advised::withheld().shown(), None);
}

#[test]
fn the_recalled_line_is_one_fixed_sentence_with_nothing_to_fill_in() {
    assert!(!RECALLED.contains(['{', '}', ':']));
    assert_eq!(RECALLED.matches('.').count(), 1);
    assert!(RECALLED.ends_with('.'));
    assert_ne!(RECALLED, WITHHELD);
}

#[test]
fn a_view_that_holds_only_some_repositories_reads_each_one_by_one_never_the_owner_s_listings() {
    let advising = Advising {
        from: AdvisoriesFrom::Owner,
        repositories: 7,
        most: 9,
    };
    let narrowed = View {
        repositories: vec![crate::Glob::new("o/pay*").unwrap()],
        ..View::default()
    };
    assert_eq!(
        advising.within(&narrowed),
        Advising {
            from: AdvisoriesFrom::Members,
            ..advising
        }
    );
    assert_eq!(advising.within(&View::default()), advising);
    for from in [AdvisoriesFrom::Shown, AdvisoriesFrom::Members] {
        let other = Advising { from, ..advising };
        assert_eq!(other.within(&narrowed), other);
    }
    assert!(View::default().holds_every());
    assert!(!narrowed.holds_every());
}

fn pull(repository: &str, visibility: Visibility) -> Pull {
    Pull {
        project: repository.to_owned(),
        repository: repository.to_owned(),
        visibility,
        number: 1,
        title: "Change".to_owned(),
        url: format!("https://example.com/{repository}/pull/1"),
        author: None,
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: Timestamp::from_second(1_000).unwrap(),
        updated: Timestamp::from_second(1_000).unwrap(),
        review: crate::queue::Review::Unasked,
        reviewers: Vec::new(),
        checks: crate::queue::Checks::Absent,
        mergeable: crate::queue::Mergeable::Unknown,
        queue: None,
        forge: None,
        head: None,
        unread: false,
        base: None,
        files: None,
        areas: Vec::new(),
        head_branch: None,
        milestone: None,
        flips: Vec::new(),
    }
}

#[test]
fn a_private_or_internal_repository_s_rows_are_printed_only_where_the_output_is_said_private() {
    let table = [
        (Printed::Withheld, [true, false, false]),
        (Printed::Configured, [true, true, true]),
        (Printed::Asked, [true, true, true]),
    ];
    for (printed, prints) in table {
        let seen = [
            Visibility::Public,
            Visibility::Internal,
            Visibility::Private,
        ]
        .map(|visibility| printed.prints(visibility));
        assert_eq!(seen, prints, "{printed:?}");
    }
    assert_eq!(
        [(true, true), (true, false), (false, true), (false, false)]
            .map(|(asked, configured)| Printed::of(asked, configured)),
        [
            Printed::Asked,
            Printed::Asked,
            Printed::Configured,
            Printed::Withheld
        ]
    );
    let pulls = vec![
        pull("o/app", Visibility::Public),
        pull("o/vault", Visibility::Private),
        pull("o/inner", Visibility::Internal),
    ];
    assert_eq!(
        [Printed::Withheld, Printed::Configured, Printed::Asked].map(Printed::prints_failures),
        [false, true, true]
    );
    let (shown, withheld) = Printed::Withheld.printed(pulls.clone());
    assert_eq!(shown, [pull("o/app", Visibility::Public)]);
    assert_eq!(
        withheld,
        [
            pull("o/vault", Visibility::Private),
            pull("o/inner", Visibility::Internal)
        ]
    );
    assert_eq!(
        Printed::Configured.printed(pulls.clone()),
        (pulls, Vec::new())
    );
}

#[test]
fn the_overviews_printed_keep_only_the_runs_unread_repositories_and_heads_whose_rows_are_printed() {
    let run = |repository: &str, visibility| crate::queue::Running {
        repository: repository.to_owned(),
        visibility,
        id: "1".to_owned(),
        url: "https://example.com/run/1".to_owned(),
        sha: "abc".to_owned(),
        pull: true,
        queued: false,
        created: Timestamp::from_second(1_000).unwrap(),
    };
    let unread = |repository: &str, visibility| Unread {
        repository: repository.to_owned(),
        visibility,
        reason: "refused".to_owned(),
    };
    let mut overviews = Overviews {
        capacity: Some(vec![
            run("o/app", Visibility::Public),
            run("o/vault", Visibility::Private),
        ]),
        unread: vec![
            unread("o/inner", Visibility::Internal),
            unread("o/app", Visibility::Public),
        ],
        bases: vec![
            based("o/inner", Visibility::Internal),
            based("o/app", Visibility::Public),
        ],
        ..Overviews::default()
    };
    let headed = |repository: &str, visibility, head: &str| {
        let mut headed = pull(repository, visibility);
        headed.head = Some(head.to_owned());
        headed
    };
    let pulls = vec![
        headed("o/app", Visibility::Public, "h-app"),
        headed("o/vault", Visibility::Private, "h-vault"),
        pull("o/unheaded", Visibility::Public),
    ];
    overviews.heads = Overviews::heads_of(&pulls);
    let (shown, _) = Printed::Withheld.printed(pulls.clone());
    let printed = Printed::Withheld.overviews(overviews.clone(), &shown);
    assert_eq!(
        printed.capacity,
        Some(vec![run("o/app", Visibility::Public)])
    );
    assert_eq!(printed.unread, [unread("o/app", Visibility::Public)]);
    assert_eq!(printed.bases, [based("o/app", Visibility::Public)]);
    assert_eq!(Printed::Withheld.sees(), Visibility::Public);
    assert_eq!(Printed::Configured.sees(), Visibility::Private);
    assert_eq!(Printed::Asked.sees(), Visibility::Private);
    assert_eq!(
        printed.heads,
        [crate::queue::Head {
            repository: "o/app".to_owned(),
            sha: "h-app".to_owned(),
        }]
    );
    assert_eq!(
        Printed::Asked.overviews(overviews.clone(), &pulls),
        overviews
    );
    assert_eq!(
        Printed::Withheld.overviews(Overviews::default(), &[]),
        Overviews::default()
    );
}

fn based(repository: &str, visibility: Visibility) -> crate::queue::Based {
    crate::queue::Based {
        repository: repository.to_owned(),
        visibility,
        branch: "main".to_owned(),
        read: crate::queue::BaseRead::Unread("refused".to_owned()),
    }
}
