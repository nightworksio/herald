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
        Surface::Queue { private: true } => 4,
        Surface::Queue { private: false } => 5,
        Surface::Digest => 6,
        Surface::Feed => 7,
        Surface::Push => 8,
        Surface::Rule => 9,
        Surface::Told => 10,
        Surface::Doctor => 11,
        Surface::Api => 12,
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
            Surface::Queue { private: true }
        ]
    );
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
