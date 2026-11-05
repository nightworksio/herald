//! An app's event read from herald's event JSON, and the schema it is
//! checked against.

#[cfg(test)]
mod app {
    use herald_core::{Kind, Problem, Standing, Step};
    use herald_sources_app::{Unread, event, identity, schema, standings};
    use jiff::Timestamp;

    fn now() -> Timestamp {
        "2027-01-15T08:00:00Z".parse().unwrap()
    }

    #[test]
    fn a_whole_event_is_read_with_every_field_it_names() {
        let read = event(
            "ci",
            br#"{
                "id": "build-42",
                "project": "herald",
                "version": "1.2.0",
                "title": "  herald\n1.2.0  ",
                "body": "  What changed  ",
                "links": {"release": "https://herald.example/releases/1.2.0"},
                "prerelease": true,
                "occurred": "2027-01-14T10:30:00Z"
            }"#,
            now(),
        )
        .unwrap();
        assert_eq!(read.identity().as_str(), "app/ci/build-42");
        assert_eq!(read.kind(), Kind::Message);
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
        )
        .unwrap();
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
        );
        assert_eq!(refused, Err(Unread::NoId));
        assert_eq!(
            Unread::NoId.to_string(),
            "the event has no id; an app gives every event one"
        );
    }

    #[test]
    fn a_field_no_event_has_is_refused_and_named() {
        let Err(Unread::NotAnEvent(why)) = event(
            "ci",
            br#"{"id": "7", "project": "herald", "title": "Deployed", "colour": "red"}"#,
            now(),
        ) else {
            panic!("an unknown field was read");
        };
        assert!(why.contains("colour"), "{why}");
    }

    #[test]
    fn bytes_that_are_not_json_are_refused() {
        let refused = event("ci", b"not json", now()).unwrap_err();
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
            now(),
        )
        .unwrap_err();
        assert_eq!(refused, Unread::Problem(Problem::LinkNotWeb));
        assert_eq!(refused.to_string(), Problem::LinkNotWeb.to_string());
    }

    #[test]
    fn an_event_with_no_project_or_title_is_refused() {
        assert_eq!(
            event("ci", br#"{"id": "7", "project": " ", "title": "t"}"#, now()),
            Err(Unread::Problem(Problem::NoProject))
        );
        assert_eq!(
            event("ci", br#"{"id": "7", "project": "p", "title": " "}"#, now()),
            Err(Unread::Problem(Problem::NoTitle))
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
            "regenerate it: cargo run -p herald-sources-app --example schema > herald-event.schema.json"
        );
    }
}
