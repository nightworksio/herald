use std::collections::BTreeMap;

use jiff::{SignedDuration, Timestamp};

use super::*;
use crate::queue::tests::pull;
use crate::queue::{Verdict, View, rows};

const NOW: i64 = 10_000_000;

fn now() -> Timestamp {
    Timestamp::from_second(NOW).unwrap()
}

fn area(name: &str, kind: AreaKind) -> Area {
    Area {
        name: name.to_owned(),
        kind,
    }
}

fn opened(number: u64, reviewers: &[&str], areas: Vec<Area>) -> Pull {
    Pull {
        number,
        url: format!("https://github.com/nightworksio/herald/pull/{number}"),
        reviewers: reviewers.iter().map(|login| (*login).to_owned()).collect(),
        areas,
        ..pull()
    }
}

fn team(members: &[&str]) -> Team {
    let members: Vec<String> = members.iter().map(|login| (*login).to_owned()).collect();
    Team::of("team", &members).unwrap()
}

fn teams(named: &[(&str, &[&str])]) -> Teams {
    named
        .iter()
        .map(|(name, members)| ((*name).to_owned(), team(members)))
        .collect()
}

fn loads(pulls: &[Pull], given: &Given, teams: &Teams, load: Load) -> Loads {
    Loads::of(
        &rows(&View::default(), pulls, now()),
        given,
        (teams, load),
        now(),
    )
}

fn count(requested: usize, reviewed: usize) -> Loaded {
    Loaded {
        requested,
        reviewed,
    }
}

fn reviewed(at: Timestamp) -> Reviewed {
    Reviewed {
        verdict: Verdict::Approved,
        decides: None,
        at,
    }
}

fn given(pull: &Pull, reviews: &[(&str, Timestamp)]) -> Given {
    let mut given = Given::default();
    let reviews: BTreeMap<String, Reviewed> = reviews
        .iter()
        .map(|(login, at)| ((*login).to_owned(), reviewed(*at)))
        .collect();
    given.add(pull, &reviews);
    given
}

#[test]
fn a_member_is_a_login_or_a_login_on_one_host_compared_without_case() {
    assert_eq!(
        TeamMember::of("Ada").unwrap(),
        TeamMember {
            login: "ada".to_owned(),
            host: None
        }
    );
    let hosted = TeamMember::of("Ada@GitLab.Example.com").unwrap();
    assert_eq!(hosted.host.as_deref(), Some("gitlab.example.com"));
    assert!(hosted.is("ADA", Some("gitlab.example.COM")));
    assert!(!hosted.is("ada", Some("github.com")));
    assert!(!hosted.is("ada", None));
    assert!(!hosted.is("bob", Some("gitlab.example.com")));
    let anywhere = TeamMember::of("ada").unwrap();
    assert!(anywhere.is("ada", Some("github.com")));
    assert!(anywhere.is("ada", None));
    assert!(TeamMember::of(&"a".repeat(MOST_LOGIN)).is_ok());
    for refused in [
        String::new(),
        "@github.com".to_owned(),
        "a@b@github.com".to_owned(),
        "ada@".to_owned(),
        "ada@git hub".to_owned(),
        "ada@git/hub".to_owned(),
        "a da".to_owned(),
        "acme/ada".to_owned(),
        "ada\u{7}".to_owned(),
        "a".repeat(MOST_LOGIN + 1),
    ] {
        assert!(TeamMember::of(&refused).is_err(), "{refused:?}");
    }
}

#[test]
fn a_team_is_named_as_an_area_is_and_holds_at_most_its_bound_of_members() {
    let most: Vec<String> = (0..MOST_TEAM_MEMBERS).map(|n| format!("m{n}")).collect();
    assert_eq!(
        Team::of("big", &most).unwrap().people().len(),
        MOST_TEAM_MEMBERS
    );
    let past: Vec<String> = (0..=MOST_TEAM_MEMBERS).map(|n| format!("m{n}")).collect();
    assert!(Team::of("big", &past).unwrap_err().contains("at most 500"));
    let unnamed = Team::of(" padded", &[]).unwrap_err();
    assert!(unnamed.contains("is no team name"), "{unnamed}");
    assert!(Team::of("ok", &["a b".to_owned()]).is_err());
    let twice = team(&["ada", "ADA", "ada@github.com"]);
    assert_eq!(twice.people(), ["ada"].into());
}

#[test]
fn a_team_counts_requests_naming_it_or_a_member_and_reviews_its_members_gave_in_the_window() {
    let one = opened(1, &["@Acme/Platform", "ada", "eve"], Vec::new());
    let two = opened(2, &["acme/other", "acme/another", "BOB"], Vec::new());
    let window = Load::default().window;
    let mut given = given(
        &one,
        &[
            ("ada", now() - window),
            ("bob", now() - window - SignedDuration::from_secs(1)),
            ("eve", now()),
        ],
    );
    given.merge(self::given(&two, &[("cy", now())]));
    let named = teams(&[("acme/platform", &["ada", "bob", "cy"])]);
    let counted = loads(&[one, two], &given, &named, Load::default());
    assert_eq!(counted.teams, [("acme/platform".to_owned(), count(3, 2))]);
    assert!(counted.reviewed);
    assert_eq!(counted.other, None);
}

#[test]
fn a_member_on_one_host_counts_only_on_that_host() {
    let mut elsewhere = opened(1, &["ada"], Vec::new());
    elsewhere.url = "https://gitlab.example.com/acme/app/-/merge_requests/1".to_owned();
    let unparsed = Pull {
        url: "not a page".to_owned(),
        ..opened(2, &["ada"], Vec::new())
    };
    let here = opened(3, &["ada"], Vec::new());
    let named = teams(&[("t", &["ada@github.com", "bob", "cy"])]);
    let counted = loads(
        &[elsewhere, unparsed, here],
        &Given::default(),
        &named,
        Load::default(),
    );
    assert_eq!(counted.teams, [("t".to_owned(), count(1, 0))]);
}

#[test]
fn a_team_of_fewer_than_the_least_team_folds_and_one_counting_nothing_is_not_shown() {
    let pulls = [opened(1, &["ada", "dan"], Vec::new())];
    let named = teams(&[
        ("three", &["ada", "bob", "cy"]),
        ("two", &["dan", "eve"]),
        ("idle", &["fay", "gus", "hal"]),
    ]);
    let counted = loads(&pulls, &Given::default(), &named, Load::default());
    assert_eq!(counted.teams, [("three".to_owned(), count(1, 0))]);
    assert_eq!(counted.other, None);
    let raised = Load {
        least_team: 4,
        ..Load::default()
    };
    let counted = loads(&pulls, &Given::default(), &named, raised);
    assert_eq!(counted.teams, []);
    assert_eq!(counted.other, Some(count(2, 0)));
}

#[test]
fn path_and_section_areas_always_show_and_person_and_name_areas_always_fold() {
    let pulls = [opened(
        1,
        &["ada"],
        vec![
            area("backend", AreaKind::Paths),
            area("Docs", AreaKind::Section),
            area("@ada", AreaKind::Person),
            area("@dan", AreaKind::Name),
        ],
    )];
    for unknown_teams in [false, true] {
        let load = Load {
            unknown_teams,
            ..Load::default()
        };
        let counted = loads(&pulls, &Given::default(), &Teams::new(), load);
        assert_eq!(
            counted.areas,
            [
                (area("Docs", AreaKind::Section), count(1, 0)),
                (area("backend", AreaKind::Paths), count(1, 0)),
            ]
        );
        assert_eq!(counted.other, None);
    }
}

#[test]
fn a_team_group_or_role_area_shows_by_its_known_members_or_as_it_is_where_unknown_teams_says() {
    let pulls = [opened(
        1,
        &["ada"],
        vec![
            area("@acme/big", AreaKind::Team),
            area("@acme/duo", AreaKind::Group),
            area("@@developer", AreaKind::Role),
            area("@acme/unknown", AreaKind::Team),
        ],
    )];
    let named = teams(&[
        ("Acme/Big", &["ada", "bob", "cy"]),
        ("acme/duo", &["dan", "eve"]),
    ]);
    let counted = loads(&pulls, &Given::default(), &named, Load::default());
    assert_eq!(
        counted.areas,
        [(area("@acme/big", AreaKind::Team), count(1, 0))]
    );
    assert_eq!(counted.other, None);
    let unknown = Load {
        unknown_teams: true,
        ..Load::default()
    };
    let counted = loads(&pulls, &Given::default(), &named, unknown);
    assert_eq!(
        counted.areas,
        [
            (area("@@developer", AreaKind::Role), count(1, 0)),
            (area("@acme/big", AreaKind::Team), count(1, 0)),
            (area("@acme/unknown", AreaKind::Team), count(1, 0)),
        ]
    );
}

#[test]
fn other_shows_once_the_people_folded_into_it_are_the_least_team_counting_each_request_once() {
    let pulls = [
        opened(
            1,
            &["ada", "bob"],
            vec![
                area("@ada", AreaKind::Person),
                area("@bob", AreaKind::Person),
            ],
        ),
        opened(2, &["cy"], vec![area("@dan", AreaKind::Name)]),
    ];
    let two = loads(&pulls, &Given::default(), &Teams::new(), Load::default());
    assert_eq!(two.other, None);
    let third = [
        pulls[0].clone(),
        opened(2, &["cy"], vec![area("@cy", AreaKind::Person)]),
    ];
    let three = loads(&third, &Given::default(), &Teams::new(), Load::default());
    assert_eq!(three.other, Some(count(3, 0)));
    let duo = teams(&[("duo", &["ada", "eve"])]);
    let folded = loads(&pulls, &Given::default(), &duo, Load::default());
    assert_eq!(folded.other, Some(count(3, 0)));
}

#[test]
fn a_folded_row_counting_nothing_adds_no_person_to_other() {
    let pulls = [opened(
        1,
        &["ada", "bob"],
        vec![
            area("@ada", AreaKind::Person),
            area("@bob", AreaKind::Person),
        ],
    )];
    let idle = teams(&[("idle", &["fay", "gus"])]);
    let counted = loads(&pulls, &Given::default(), &idle, Load::default());
    assert_eq!(counted.other, None);
}

#[test]
fn a_view_read_live_counts_no_review_and_says_so() {
    let one = opened(1, &[], vec![area("backend", AreaKind::Paths)]);
    let mut given = given(&one, &[("ada", now())]);
    let counted = loads(
        std::slice::from_ref(&one),
        &given,
        &Teams::new(),
        Load::default(),
    );
    assert_eq!(counted.areas[0].1, count(0, 1));
    assert!(counted.reviewed);
    given.merge(Given {
        live: true,
        ..Given::default()
    });
    let counted = loads(&[one], &given, &Teams::new(), Load::default());
    assert!(!counted.reviewed);
}

#[test]
fn a_load_is_counted_only_from_the_rows_it_is_given() {
    let shown = opened(1, &["ada"], vec![area("backend", AreaKind::Paths)]);
    let hidden = opened(2, &["ada"], vec![area("secret", AreaKind::Paths)]);
    let given = given(&hidden, &[("bob", now())]);
    let named = teams(&[("t", &["ada", "bob", "cy"])]);
    let counted = loads(&[shown], &given, &named, Load::default());
    assert_eq!(counted.teams, [("t".to_owned(), count(1, 0))]);
    assert_eq!(
        counted.areas,
        [(area("backend", AreaKind::Paths), count(1, 0))]
    );
    assert_eq!(Load::default().least_team, LEAST_TEAM);
    assert_eq!(Load::default().window, LOAD_WINDOW);
    assert!(!Load::default().unknown_teams);
}
