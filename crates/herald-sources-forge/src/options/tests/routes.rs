use herald_core::{Reach, RouteId};

use super::*;

fn routed(routes: &[&str], routes_only: Option<bool>) -> Override {
    Override {
        routes: Some(routes.iter().map(|route| (*route).to_owned()).collect()),
        routes_only,
        ..Override::default()
    }
}

#[test]
fn the_narrowest_override_naming_routes_gives_them_and_whether_they_are_the_only_ones() {
    let options = SourceOptions {
        repositories: BTreeMap::from([
            ("plugin-*".to_owned(), routed(&["plugins"], Some(true))),
            ("plugin-sonarr".to_owned(), routed(&["arr"], None)),
            (
                "web*".to_owned(),
                Override {
                    routes_only: Some(false),
                    ..Override::default()
                },
            ),
        ]),
        ..owner("lemonfiber")
    };
    assert_eq!(options.check(Forge::GitHub), Ok(()));
    assert_eq!(
        options.reach_of("plugin-radarr"),
        Reach {
            routes: vec![RouteId::new("plugins")],
            only: true,
        }
    );
    assert_eq!(
        options.reach_of("plugin-sonarr"),
        Reach {
            routes: vec![RouteId::new("arr")],
            only: true,
        }
    );
    assert_eq!(options.reach_of("website"), Reach::default());
    assert_eq!(options.reach_of("herald"), Reach::default());
    assert_eq!(
        options.routed(),
        [
            (
                "repositories.\"plugin-*\".routes".to_owned(),
                "plugins".to_owned()
            ),
            (
                "repositories.\"plugin-sonarr\".routes".to_owned(),
                "arr".to_owned()
            ),
        ]
    );
}

#[test]
fn two_overrides_as_narrow_as_each_other_tie_on_routes_and_routes_only() {
    let both = |one: Override, other: Override| {
        SourceOptions {
            repositories: BTreeMap::from([
                ("plugin-*".to_owned(), one),
                ("*-sonarr".to_owned(), other),
            ]),
            ..owner("a")
        }
        .check(Forge::GitHub)
    };
    assert_eq!(
        both(routed(&["a"], None), routed(&["b"], None)),
        Err("repositories \"*-sonarr\" and \"plugin-*\" both set routes, are as narrow as each other and could both name one repository; make one narrower".to_owned())
    );
    let only = |only| Override {
        routes_only: Some(only),
        ..Override::default()
    };
    assert_eq!(
        both(only(true), only(false)),
        Err("repositories \"*-sonarr\" and \"plugin-*\" both set routes_only, are as narrow as each other and could both name one repository; make one narrower".to_owned())
    );
    assert_eq!(both(routed(&["a"], None), only(true)), Ok(()));
}
