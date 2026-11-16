use super::*;
use crate::event::tests::event;

pub(crate) fn origin() -> Origin {
    Origin::of_member(
        "github",
        "github.com",
        &Listed {
            id: "7".to_owned(),
            name: "herald".to_owned(),
            path: "nightworksio/herald".to_owned(),
            topics: vec!["Release".to_owned()],
            properties: BTreeMap::from([("team".to_owned(), vec!["Core".to_owned()])]),
            visibility: Visibility::Internal,
            archived: false,
            fork: false,
            template: false,
            language: None,
        },
    )
}

#[test]
fn a_member_s_origin_holds_its_listing() {
    let origin = origin();
    assert_eq!(origin.forge, "github");
    assert_eq!(origin.host, "github.com");
    assert_eq!(origin.owner, "nightworksio");
    assert_eq!(origin.repository, "nightworksio/herald");
    assert_eq!(origin.id, "7");
    assert_eq!(origin.visibility, Visibility::Internal);
    assert_eq!(origin.source, None);
    assert_eq!(origin.clone().checked(), Ok(origin));
}

#[test]
fn a_package_s_origin_is_public_with_no_topics_or_properties() {
    let origin = Origin::of_package(
        "npm",
        "registry.npmjs.org",
        "nightworks",
        "@nightworks/herald",
    );
    assert_eq!(origin.repository, "@nightworks/herald");
    assert_eq!(origin.id, "@nightworks/herald");
    assert_eq!(origin.owner, "nightworks");
    assert_eq!(origin.visibility, Visibility::Public);
    assert!(origin.topics.is_empty() && origin.properties.is_empty());
    assert_eq!(origin.clone().checked(), Ok(origin));
    let unscoped = Origin::of_package("crates-io", "crates.io", "", "herald");
    assert_eq!(unscoped.clone().checked(), Ok(unscoped));
}

#[test]
fn an_origin_no_source_could_give_is_refused_by_its_first_field() {
    let long = "a".repeat(FIELD + 1);
    for (origin, problem) in [
        (
            Origin {
                topics: vec![long.clone()],
                ..origin()
            },
            Problem::OriginTooLong,
        ),
        (
            Origin {
                properties: BTreeMap::from([("team".to_owned(), vec![long])]),
                ..origin()
            },
            Problem::OriginTooLong,
        ),
        (
            Origin {
                forge: "GitHub".to_owned(),
                ..origin()
            },
            Problem::OriginForge,
        ),
        (
            Origin {
                forge: "-github".to_owned(),
                ..origin()
            },
            Problem::OriginForge,
        ),
        (
            Origin {
                forge: "github-".to_owned(),
                ..origin()
            },
            Problem::OriginForge,
        ),
        (
            Origin {
                forge: String::new(),
                ..origin()
            },
            Problem::OriginForge,
        ),
        (
            Origin {
                owner: "../etc".to_owned(),
                ..origin()
            },
            Problem::OriginPath,
        ),
        (
            Origin {
                repository: "a b".to_owned(),
                ..origin()
            },
            Problem::OriginPath,
        ),
        (
            Origin {
                id: " ".to_owned(),
                ..origin()
            },
            Problem::OriginId,
        ),
    ] {
        assert_eq!(origin.clone().checked(), Err(problem), "{origin:?}");
    }
    let full = Origin {
        topics: vec!["a".repeat(FIELD)],
        ..origin()
    };
    assert_eq!(full.clone().checked(), Ok(full));
}

#[test]
fn an_origin_s_host_is_a_domain_or_localhost_with_a_port_where_it_gives_one() {
    for host in [
        "https://github.com",
        "forge.example:0",
        "forge.example:http",
    ] {
        let origin = Origin {
            host: host.to_owned(),
            ..origin()
        };
        assert_eq!(origin.checked(), Err(Problem::OriginHost), "{host}");
    }
    for host in [
        "forge.example:3000",
        "localhost",
        "localhost:8080",
        "10.0.0.1",
    ] {
        let origin = Origin {
            host: host.to_owned(),
            ..origin()
        };
        assert_eq!(origin.clone().checked(), Ok(origin));
    }
}

#[test]
fn topics_and_properties_are_held_ignoring_case() {
    let origin = origin();
    assert!(origin.holds_topics(&[]));
    assert!(origin.holds_topics(&["other".to_owned(), "release".to_owned()]));
    assert!(!origin.holds_topics(&["other".to_owned()]));
    let wanted = |name: &str, values: &[&str]| {
        BTreeMap::from([(
            name.to_owned(),
            values.iter().map(|value| (*value).to_owned()).collect(),
        )])
    };
    assert!(origin.holds_properties(&BTreeMap::new()));
    assert!(origin.holds_properties(&wanted("team", &["core"])));
    assert!(origin.holds_properties(&wanted("team", &[])));
    assert!(!origin.holds_properties(&wanted("team", &["media"])));
    assert!(!origin.holds_properties(&wanted("tier", &[])));
    let emptied = Origin {
        properties: BTreeMap::from([("team".to_owned(), Vec::new())]),
        ..origin
    };
    assert!(!emptied.holds_properties(&wanted("team", &[])));
}

#[test]
fn an_event_carries_its_origin_and_the_source_it_came_through() {
    let source = SourceName::new("nightworksio");
    assert_eq!(event().through(&source).origin(), None);
    let from = event().from_origin(origin()).through(&source);
    assert_eq!(
        from.origin().and_then(|origin| origin.source.as_ref()),
        Some(&source)
    );
    assert_eq!(from.bounded_by(), Some(Visibility::Internal));
    assert_eq!(
        from.clone().seen_as(Visibility::Public).bounded_by(),
        Some(Visibility::Public)
    );
    assert_eq!(event().bounded_by(), None);
    let json = serde_json::to_string(&from).unwrap();
    assert!(json.contains("\"source\":\"nightworksio\""), "{json}");
    let read: crate::Event = serde_json::from_str(&json).unwrap();
    assert_eq!(read, from);
}

#[test]
fn each_origin_problem_names_the_origin() {
    for problem in [
        Problem::OriginTooLong,
        Problem::OriginForge,
        Problem::OriginHost,
        Problem::OriginPath,
        Problem::OriginId,
    ] {
        assert!(problem.to_string().contains("origin"), "{problem:?}");
    }
}
