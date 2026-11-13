//! An app's event read from herald's event JSON, and the schema it is
//! checked against.

#[cfg(test)]
mod app {
    use herald_core::{Attribute, Kind, KindSpecs, Problem, Standing, Step};
    use herald_sources_app::{UnreadEvent, event, identity, schema, standings};
    use jiff::Timestamp;

    fn now() -> Timestamp {
        "2027-01-15T08:00:00Z".parse().unwrap()
    }

    #[test]
    fn an_event_longer_than_an_app_may_send_is_refused_unread() {
        let long = vec![b' '; herald_sources_app::MOST + 1];
        let refused = event(
            "ci",
            &long,
            jiff::Timestamp::UNIX_EPOCH,
            &KindSpecs::default(),
        )
        .unwrap_err();
        assert_eq!(
            refused.to_string(),
            format!(
                "the event is longer than {} bytes; send a shorter one",
                herald_sources_app::MOST
            )
        );
    }

    #[test]
    fn an_event_as_long_as_an_app_may_send_is_read() {
        let mut longest = br#"{"id": "build-42", "project": "herald", "title": "herald"}"#.to_vec();
        longest.resize(herald_sources_app::MOST, b' ');
        let read = event("ci", &longest, now(), &KindSpecs::default()).unwrap();
        assert_eq!(read.identity().as_str(), identity("ci", "build-42"));
    }

    #[test]
    fn a_whole_event_is_read_with_every_field_it_names() {
        let read = event(
            "ci",
            br#"{
                "id": "build-42",
                "kind": "release",
                "project": "herald",
                "attributes": {"version": "1.2.0", "prerelease": true},
                "title": "  herald\n1.2.0  ",
                "body": "  What changed  ",
                "links": {"release": "https://herald.example/releases/1.2.0"},
                "occurred": "2027-01-14T10:30:00Z"
            }"#,
            now(),
            &KindSpecs::default(),
        )
        .unwrap();
        assert_eq!(read.identity().as_str(), "app/ci/build-42");
        assert_eq!(read.kind(), &Kind::RELEASE);
        assert_eq!(read.project(), "herald");
        assert_eq!(read.version(), Some("1.2.0"));
        assert_eq!(read.title(), "herald 1.2.0");
        assert_eq!(read.body(), "What changed");
        assert_eq!(
            read.link("release"),
            Some("https://herald.example/releases/1.2.0")
        );
        assert!(read.prerelease());
        assert_eq!(
            read.occurred(),
            "2027-01-14T10:30:00Z".parse::<Timestamp>().unwrap()
        );
    }

    #[test]
    fn an_event_naming_only_its_id_project_and_title_happened_when_it_was_read() {
        let read = event(
            "ci",
            br#"{"id": "7", "project": "herald", "title": "Deployed"}"#,
            now(),
            &KindSpecs::default(),
        )
        .unwrap();
        assert_eq!(read.kind(), &Kind::MESSAGE);
        assert_eq!(read.version(), None);
        assert_eq!(read.body(), "");
        assert_eq!(read.first_link(), None);
        assert!(!read.prerelease());
        assert_eq!(read.occurred(), now());
    }

    #[test]
    fn the_identity_is_the_app_s_name_and_its_trimmed_id() {
        let read = event(
            "deploys",
            br#"{"id": "  7  ", "project": "herald", "title": "Deployed"}"#,
            now(),
            &KindSpecs::default(),
        )
        .unwrap();
        assert_eq!(read.identity().as_str(), identity("deploys", "7"));
        assert_eq!(identity("deploys", "7"), "app/deploys/7");
    }

    #[test]
    fn an_event_with_no_id_is_refused() {
        let refused = event(
            "ci",
            br#"{"id": "  ", "project": "herald", "title": "Deployed"}"#,
            now(),
            &KindSpecs::default(),
        );
        assert_eq!(refused, Err(UnreadEvent::NoId));
        assert_eq!(
            UnreadEvent::NoId.to_string(),
            "the event has no id; an app gives every event one"
        );
    }

    #[test]
    fn a_field_no_event_has_is_refused_and_named() {
        let Err(UnreadEvent::NotAnEvent(why)) = event(
            "ci",
            br#"{"id": "7", "project": "herald", "title": "Deployed", "colour": "red"}"#,
            now(),
            &KindSpecs::default(),
        ) else {
            panic!("an unknown field was read");
        };
        assert!(why.contains("colour"), "{why}");
    }

    #[test]
    fn a_kind_herald_does_not_know_is_refused_and_named() {
        let refused = event(
            "ci",
            br#"{"id": "7", "kind": "deploy", "project": "herald", "title": "Deployed"}"#,
            now(),
            &KindSpecs::default(),
        )
        .unwrap_err();
        assert_eq!(
            refused.to_string(),
            "herald knows no kind \"deploy\"; send a kind it ships, or one the config declares under [kinds.deploy]"
        );
    }

    #[test]
    fn a_declared_kind_is_read_with_its_attributes_and_refused_without_them() {
        let deploy = herald_core::KindSpec::new(
            Kind::new("deploy").unwrap(),
            "A deploy.",
            false,
            Vec::new(),
            [(
                "region".to_owned(),
                herald_core::AttributeSpec {
                    ty: herald_core::AttributeType::Text,
                    required: true,
                },
            )]
            .into(),
        );
        let kinds = KindSpecs::default().with(deploy);
        let read = event(
            "ci",
            br#"{"id": "7", "kind": "deploy", "project": "herald", "title": "Deployed", "attributes": {"region": "eu"}}"#,
            now(),
            &kinds,
        )
        .unwrap();
        assert_eq!(
            read.attribute("region"),
            Some(&Attribute::Text("eu".to_owned()))
        );
        let refused = event(
            "ci",
            br#"{"id": "7", "kind": "deploy", "project": "herald", "title": "Deployed"}"#,
            now(),
            &kinds,
        )
        .unwrap_err();
        assert_eq!(
            refused.to_string(),
            "attributes.region is missing; a deploy event carries it"
        );
    }

    #[test]
    fn bytes_that_are_not_json_are_refused() {
        let refused = event("ci", b"not json", now(), &KindSpecs::default()).unwrap_err();
        assert!(
            refused
                .to_string()
                .starts_with("the event is not herald's event JSON: "),
            "{refused}"
        );
    }

    #[test]
    fn a_link_that_is_not_a_web_url_is_refused_with_the_event_s_own_problem() {
        let refused = event(
            "ci",
            br#"{"id": "7", "project": "herald", "title": "Deployed", "links": {"release": "ftp://x"}}"#,
            now(), &KindSpecs::default(),
        )
        .unwrap_err();
        assert_eq!(refused, UnreadEvent::Problem(Problem::LinkNotWeb));
        assert_eq!(refused.to_string(), Problem::LinkNotWeb.to_string());
    }

    #[test]
    fn an_event_with_no_project_or_title_is_refused() {
        assert_eq!(
            event(
                "ci",
                br#"{"id": "7", "project": " ", "title": "t"}"#,
                now(),
                &KindSpecs::default()
            ),
            Err(UnreadEvent::Problem(Problem::NoProject))
        );
        assert_eq!(
            event(
                "ci",
                br#"{"id": "7", "project": "p", "title": " "}"#,
                now(),
                &KindSpecs::default()
            ),
            Err(UnreadEvent::Problem(Problem::NoTitle))
        );
    }

    #[test]
    fn where_an_event_stands_reads_as_json_route_by_route() {
        let answer = standings(
            "7",
            "herald",
            &[
                ("deployed", Standing::Announced),
                (
                    "elsewhere",
                    Standing::Stopped {
                        remaining: vec![Step::Post],
                    },
                ),
            ],
        );
        assert_eq!(
            answer,
            r#"{"id":"7","project":"herald","routes":[{"route":"deployed","standing":"announced"},{"route":"elsewhere","standing":"stopped","remaining":["post"]}]}"#
        );
    }

    #[test]
    fn the_committed_schema_is_the_one_the_types_generate() {
        assert_eq!(
            schema(),
            include_str!("../../../herald-event.schema.json"),
            "regenerate it: cargo run -p herald-sources-app --example event-schema > herald-event.schema.json"
        );
    }

    fn mistakes(event: &serde_json::Value) -> usize {
        let schema: serde_json::Value = serde_json::from_str(&schema()).unwrap();
        jsonschema::options()
            .should_validate_formats(true)
            .build(&schema)
            .unwrap()
            .iter_errors(event)
            .count()
    }

    #[test]
    fn the_schema_takes_the_readme_s_event_and_refuses_what_the_reader_refuses() {
        let readme = include_str!("../../../README.md");
        let example = r#"{"id": "7", "project": "herald", "title": "Deployed"}"#;
        assert!(readme.contains(example));
        assert_eq!(mistakes(&serde_json::from_str(example).unwrap()), 0);
        let whole = serde_json::json!({
            "id": "deploy/7", "project": "night works", "title": "Deployed",
            "links": { "release": "https://example.org/r" },
            "occurred": "2027-01-15T08:00:00Z"
        });
        assert_eq!(mistakes(&whole), 0);
        for refused in [
            serde_json::json!({ "id": " ", "project": "herald", "title": "Deployed" }),
            serde_json::json!({ "id": "7", "project": "", "title": "Deployed" }),
            serde_json::json!({ "id": "7", "project": "herald", "title": "\t" }),
            serde_json::json!({ "id": "7", "project": "herald", "title": "t", "links": { "r": "ftp://example.org" } }),
            serde_json::json!({ "id": "7", "project": "herald", "title": "t", "links": { " ": "https://example.org" } }),
            serde_json::json!({ "id": "7", "project": "herald", "title": "t", "occurred": "yesterday" }),
        ] {
            assert_ne!(mistakes(&refused), 0, "{refused}");
        }
    }

    #[test]
    fn an_identity_is_the_app_and_the_id_without_white_space_around_it() {
        assert_eq!(identity("ci", " deploy/7 "), "app/ci/deploy/7");
    }
}
