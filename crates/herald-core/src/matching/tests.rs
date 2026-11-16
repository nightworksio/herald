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
