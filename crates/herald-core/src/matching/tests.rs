use super::*;
use crate::event::tests::event;
use crate::origin::tests::origin;

fn globs(texts: &[&str]) -> Vec<Glob> {
    texts.iter().map(|text| Glob::new(text).unwrap()).collect()
}

fn names(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| (*text).to_owned()).collect()
}

#[test]
fn an_empty_match_takes_every_event_with_an_origin_or_without() {
    let matching = Match::default();
    assert!(!matching.reads_origin());
    assert!(matching.takes(&event()));
    assert!(matching.takes(&event().from_origin(origin())));
    assert!(matching.admits("anything", None));
}

#[test]
fn projects_narrow_by_glob_whether_or_not_an_event_has_an_origin() {
    let matching = Match {
        projects: globs(&["PHP-*"]),
        ..Match::default()
    };
    assert!(!matching.reads_origin());
    assert!(matching.takes(&event()));
    assert!(!matching.takes(&event().of_project("herald").unwrap()));
    assert!(matching.admits("php-gate", None));
    assert!(!matching.admits("herald", Some(&origin())));
}

/// That each of `cases` takes the sample origin as it says, and an event
/// with no origin never.
fn narrows(cases: Vec<(Match, bool)>) {
    let origin = origin();
    for (matching, takes) in cases {
        assert!(matching.reads_origin(), "{matching:?}");
        assert_eq!(
            matching.takes(&event().from_origin(origin.clone())),
            takes,
            "{matching:?}"
        );
        assert_eq!(
            matching.admits("php-mutation-gate", Some(&origin)),
            takes,
            "{matching:?}"
        );
        assert!(!matching.takes(&event()), "{matching:?}");
        assert!(matching.admits("php-mutation-gate", None), "{matching:?}");
    }
}

#[test]
fn repositories_and_owners_narrow_by_glob() {
    let by = |repositories: &[&str], owners: &[&str]| Match {
        repositories: globs(repositories),
        owners: globs(owners),
        ..Match::default()
    };
    narrows(vec![
        (by(&["*/her*"], &[]), true),
        (by(&["*/plugin-*"], &[]), false),
        (by(&[], &["NightWorksIO"]), true),
        (by(&[], &["lemonfiber"]), false),
    ]);
}

#[test]
fn forges_hosts_topics_and_properties_narrow_ignoring_case() {
    let by = |forges: &[&str], hosts: &[&str], topics: &[&str]| Match {
        forges: names(forges),
        hosts: names(hosts),
        topics: names(topics),
        ..Match::default()
    };
    let properties = |value: &str| Match {
        properties: BTreeMap::from([("team".to_owned(), names(&[value]))]),
        ..Match::default()
    };
    narrows(vec![
        (by(&["GitHub"], &[], &[]), true),
        (by(&["gitlab"], &[], &[]), false),
        (by(&[], &["github.com"], &[]), true),
        (by(&[], &["ghe.example"], &[]), false),
        (by(&[], &[], &["release"]), true),
        (by(&[], &[], &["media"]), false),
        (properties("core"), true),
        (properties("media"), false),
    ]);
}

/// The sample event, carrying `attempt` as a number beside its version and
/// pre-release flag.
fn attempted(attempt: u64) -> Event {
    let mut draft = crate::event::tests::draft();
    draft
        .attributes
        .insert("attempt".to_owned(), Attribute::Number(attempt.into()));
    Event::normalise(draft).unwrap()
}

fn values(pairs: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    pairs
        .iter()
        .map(|(name, values)| ((*name).to_owned(), names(values)))
        .collect()
}

#[test]
fn attributes_narrow_by_text_number_or_flag_ignoring_case() {
    let by = |pairs: &[(&str, &[&str])]| Match {
        attributes: values(pairs),
        ..Match::default()
    };
    let event = attempted(2);
    for (pairs, takes) in [
        (&[("version", &["V0.1.0"][..])][..], true),
        (&[("version", &["v0.2.0", "v0.1.0"][..])][..], true),
        (&[("version", &["v0.2.0"][..])][..], false),
        (&[("prerelease", &["false"][..])][..], true),
        (&[("prerelease", &["true"][..])][..], false),
        (&[("attempt", &["2"][..])][..], true),
        (&[("attempt", &["1"][..])][..], false),
        (&[("conclusion", &["failure"][..])][..], false),
        (
            &[("attempt", &["2"][..]), ("version", &["v9"][..])][..],
            false,
        ),
    ] {
        let matching = by(pairs);
        assert!(!matching.reads_origin(), "{pairs:?}");
        assert_eq!(matching.takes(&event), takes, "{pairs:?}");
        assert!(matching.admits("anything", Some(&origin())), "{pairs:?}");
    }
}

#[test]
fn nothing_not_takes_is_taken() {
    let excluding = |not: Match| Match {
        not: Some(Box::new(not)),
        ..Match::default()
    };
    let by_attempt = excluding(Match {
        attributes: values(&[("attempt", &["2"])]),
        ..Match::default()
    });
    assert!(!by_attempt.takes(&attempted(2)));
    assert!(by_attempt.takes(&attempted(1)));
    assert!(by_attempt.admits("php-mutation-gate", None));
    assert!(by_attempt.admits("php-mutation-gate", Some(&origin())));
    let by_project = excluding(Match {
        projects: globs(&["php-*"]),
        ..Match::default()
    });
    assert!(!by_project.takes(&attempted(1)));
    assert!(by_project.takes(&attempted(1).of_project("herald").unwrap()));
    assert!(!by_project.admits("php-mutation-gate", None));
    assert!(by_project.admits("herald", None));
    let by_owner = excluding(Match {
        owners: globs(&["nightworksio"]),
        ..Match::default()
    });
    assert!(!by_owner.takes(&attempted(1).from_origin(origin())));
    assert!(by_owner.takes(&attempted(1)));
    assert!(!by_owner.admits("php-mutation-gate", Some(&origin())));
    assert!(by_owner.admits("php-mutation-gate", None));
    let narrowed = Match {
        projects: globs(&["herald"]),
        ..by_attempt
    };
    assert!(!narrowed.takes(&attempted(1)));
    assert!(!narrowed.admits("php-mutation-gate", None));
}

/// The sample event from the sample origin, its areas `areas`.
fn zoned(areas: Option<&[&str]>) -> Event {
    let mut origin = origin();
    origin.areas = areas.map(names);
    event().from_origin(origin)
}

#[test]
fn areas_take_an_event_in_one_of_them_and_never_one_whose_areas_are_not_known() {
    let matching = Match {
        areas: globs(&["WEB*"]),
        ..Match::default()
    };
    assert!(matching.reads_origin());
    assert!(matching.reads_areas());
    assert!(matching.takes(&zoned(Some(&["docs", "web-app"]))));
    assert!(!matching.takes(&zoned(Some(&["docs"]))));
    assert!(!matching.takes(&zoned(Some(&[]))));
    assert!(!matching.takes(&zoned(None)));
    assert!(!matching.takes(&event()));
    assert!(matching.admits("php-mutation-gate", Some(&origin())));
    assert!(!Match::default().reads_areas());
}

#[test]
fn areas_under_not_refuse_an_event_in_one_of_them_or_whose_areas_are_not_known() {
    let matching = Match {
        not: Some(Box::new(Match {
            areas: globs(&["secret"]),
            ..Match::default()
        })),
        ..Match::default()
    };
    assert!(matching.reads_areas());
    assert!(!matching.reads_origin());
    assert!(!matching.takes(&zoned(Some(&["web", "secret"]))));
    assert!(!matching.takes(&zoned(None)));
    assert!(matching.takes(&zoned(Some(&["web"]))));
    assert!(matching.takes(&zoned(Some(&[]))));
    assert!(matching.takes(&event()));
    assert!(matching.admits("php-mutation-gate", Some(&origin())));
    assert!(matching.admits("php-mutation-gate", None));
}
