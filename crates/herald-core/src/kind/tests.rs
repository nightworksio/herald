use super::*;

fn spec(ty: AttributeType, required: bool) -> KindSpec {
    KindSpec::new(
        Kind::new("deploy").unwrap(),
        "A deploy.",
        false,
        vec![LinkSpec::named("site")],
        BTreeMap::from([("region".to_owned(), AttributeSpec { ty, required })]),
    )
}

fn one(name: &str, attribute: Attribute) -> BTreeMap<String, Attribute> {
    BTreeMap::from([(name.to_owned(), attribute)])
}

#[test]
fn a_kind_is_words_of_lowercase_letters_and_digits_joined_by_a_dash() {
    for name in ["release", "package-version", "v2", "a-1-b"] {
        assert_eq!(Kind::new(name).unwrap().as_str(), name);
    }
    for name in ["", "-", "a-", "-a", "a--b", "Release", "a_b", "a b", "é"] {
        assert_eq!(
            Kind::new(name).unwrap_err(),
            format!(
                "\"{name}\" is not a kind; name it in lowercase letters and digits, words joined by '-'"
            )
        );
    }
}

#[test]
fn a_kind_reads_and_writes_as_its_name() {
    let kind: Kind = serde_json::from_str("\"feed-entry\"").unwrap();
    assert_eq!(kind, Kind::FEED_ENTRY);
    assert_eq!(serde_json::to_string(&kind).unwrap(), "\"feed-entry\"");
    assert_eq!(kind.to_string(), "feed-entry");
    assert!(serde_json::from_str::<Kind>("\"Feed\"").is_err());
    assert_eq!(String::from(Kind::new("deploy").unwrap()), "deploy");
}

#[test]
fn a_kind_s_schema_is_a_string_of_its_pattern() {
    let schema = schemars::schema_for!(Kind);
    assert_eq!(Kind::schema_name(), "Kind");
    assert_eq!(schema.get("type"), Some(&"string".into()));
    assert_eq!(
        schema.get("pattern"),
        Some(&"^[a-z0-9]+(-[a-z0-9]+)*$".into())
    );
}

#[test]
fn an_attribute_reads_as_a_flag_a_number_or_text() {
    let read: BTreeMap<String, Attribute> =
        serde_json::from_str(r#"{"a": true, "b": 2, "c": "x"}"#).unwrap();
    assert_eq!(read["a"], Attribute::Flag(true));
    assert_eq!(read["b"], Attribute::Number(2.into()));
    assert_eq!(read["c"], Attribute::Text("x".to_owned()));
}

#[test]
fn a_versioned_kind_carries_a_required_version_and_an_optional_prerelease() {
    let versioned = KindSpec::new(Kind::RELEASE, "", true, Vec::new(), BTreeMap::new());
    assert!(versioned.versioned);
    assert_eq!(
        versioned.attributes[VERSION],
        AttributeSpec {
            ty: AttributeType::Text,
            required: true
        }
    );
    assert_eq!(
        versioned.attributes[PRERELEASE],
        AttributeSpec {
            ty: AttributeType::Flag,
            required: false
        }
    );
    assert!(versioned.check(&super::versioned("1.0", false)).is_ok());
    assert_eq!(
        versioned.check(&BTreeMap::new()).unwrap_err(),
        "attributes.version is missing; release events carry it"
    );
    let unversioned = spec(AttributeType::Text, false);
    assert!(!unversioned.versioned);
    assert!(!unversioned.attributes.contains_key(VERSION));
    assert_eq!(unversioned.description, "A deploy.");
    assert_eq!(unversioned.links, [LinkSpec::named("site")]);
}

#[test]
fn herald_ships_seven_kinds_and_only_releases_and_package_versions_are_versioned() {
    let shipped = KindSpec::shipped();
    let versioned: Vec<(&str, bool)> = shipped
        .iter()
        .map(|spec| (spec.kind.as_str(), spec.versioned))
        .collect();
    assert_eq!(
        versioned,
        [
            ("release", true),
            ("package-version", true),
            ("feed-entry", false),
            ("message", false),
            ("run", false),
            ("alert", false),
            ("pull-request", false)
        ]
    );
    assert_eq!(shipped[2].links, [LinkSpec::named("entry")]);
    assert_eq!(
        shipped[3].links,
        [LinkSpec::named("release"), LinkSpec::named("changelog")]
    );
    let signalled = AttributeSpec {
        ty: AttributeType::Text,
        required: false,
    };
    let signals: Vec<Option<&AttributeSpec>> = shipped
        .iter()
        .map(|spec| spec.attributes.get(crate::SIGNAL))
        .collect();
    assert_eq!(
        signals,
        [
            Some(&signalled),
            Some(&signalled),
            None,
            None,
            Some(&signalled),
            Some(&signalled),
            Some(&signalled)
        ]
    );
    for name in [crate::PACKAGE, crate::PACKAGE_TYPE] {
        let packaged: Vec<Option<&AttributeSpec>> = shipped
            .iter()
            .map(|spec| spec.attributes.get(name))
            .collect();
        assert_eq!(
            packaged[..5],
            [None, Some(&signalled), None, None, None],
            "{name}"
        );
    }
    assert_eq!(
        [crate::PACKAGE, crate::PACKAGE_TYPE].map(|name| shipped[5].attributes.contains_key(name)),
        [true, false]
    );
    let mut tagged = crate::versioned("v1", false);
    tagged.insert(crate::SIGNAL.to_owned(), Attribute::Text("tag".to_owned()));
    assert_eq!(shipped[0].check(&tagged), Ok(()));
    assert!(
        shipped[2]
            .check(&one(crate::SIGNAL, Attribute::Text("tag".to_owned())))
            .is_err()
    );
}

#[test]
fn a_run_names_its_action_workflow_and_source_and_links_its_page() {
    let run = &KindSpec::shipped()[4];
    assert_eq!(run.kind, Kind::RUN);
    assert_eq!(run.links, [LinkSpec::named("run")]);
    let typed: Vec<(&str, AttributeType, bool)> = run
        .attributes
        .iter()
        .map(|(name, spec)| (name.as_str(), spec.ty, spec.required))
        .collect();
    assert_eq!(
        typed,
        [
            ("action", AttributeType::Text, true),
            ("actor", AttributeType::Text, false),
            ("attempt", AttributeType::Number, false),
            ("branch", AttributeType::Text, false),
            ("commit", AttributeType::Text, false),
            ("conclusion", AttributeType::Text, false),
            ("edited", AttributeType::Flag, false),
            ("from", AttributeType::Text, true),
            ("number", AttributeType::Number, false),
            ("signal", AttributeType::Text, false),
            ("trigger", AttributeType::Text, false),
            ("withdrawn", AttributeType::Flag, false),
            ("workflow", AttributeType::Text, true),
        ]
    );
    let finished = BTreeMap::from([
        (
            crate::ACTION.to_owned(),
            Attribute::Text("completed".to_owned()),
        ),
        (crate::WORKFLOW.to_owned(), Attribute::Text("CI".to_owned())),
        (
            crate::FROM.to_owned(),
            Attribute::Text("actions".to_owned()),
        ),
        (crate::ATTEMPT.to_owned(), Attribute::Number(2.into())),
    ]);
    assert_eq!(run.check(&finished), Ok(()));
    assert!(
        run.check(&one(crate::WORKFLOW, Attribute::Text("CI".to_owned())))
            .is_err()
    );
}

#[test]
fn an_alert_names_its_tool_and_number_and_never_what_it_found() {
    let shipped = KindSpec::shipped();
    let alert = &shipped[5];
    assert_eq!(alert.kind, Kind::ALERT);
    let private: Vec<bool> = shipped.iter().map(|spec| spec.private).collect();
    assert_eq!(private, [false, false, false, false, false, true, false]);
    assert_eq!(alert.links, [LinkSpec::named("alert")]);
    let required: Vec<&str> = alert
        .attributes
        .iter()
        .filter(|(_, spec)| spec.required)
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(required, ["action", "number", "tool"]);
    let names: Vec<&str> = alert.attributes.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "action",
            "ecosystem",
            "edited",
            "number",
            "package",
            "path",
            "resolution",
            "rule",
            "severity",
            "signal",
            "tool",
            "withdrawn"
        ]
    );
}

#[test]
fn each_type_holds_its_own_attributes_only() {
    let cases = [
        (AttributeType::Text, Attribute::Text("eu".to_owned()), None),
        (
            AttributeType::Text,
            Attribute::Flag(true),
            Some("attributes.region is not text; deploy events carry region as text"),
        ),
        (AttributeType::Flag, Attribute::Flag(false), None),
        (
            AttributeType::Flag,
            Attribute::Number(1.into()),
            Some("attributes.region is not a flag; deploy events carry region as a flag"),
        ),
        (AttributeType::Number, Attribute::Number(1.into()), None),
        (
            AttributeType::Number,
            Attribute::Text("1".to_owned()),
            Some("attributes.region is not a number; deploy events carry region as a number"),
        ),
        (
            AttributeType::Url,
            Attribute::Text("https://example.org".to_owned()),
            None,
        ),
        (
            AttributeType::Url,
            Attribute::Text("ftp://example.org".to_owned()),
            Some(
                "attributes.region is not an https or http address; deploy events carry region as an https or http address",
            ),
        ),
        (
            AttributeType::Url,
            Attribute::Flag(true),
            Some(
                "attributes.region is not an https or http address; deploy events carry region as an https or http address",
            ),
        ),
    ];
    for (ty, attribute, refused) in cases {
        assert_eq!(
            spec(ty, true)
                .check(&one("region", attribute))
                .err()
                .as_deref(),
            refused,
            "{ty:?}"
        );
    }
}

#[test]
fn an_optional_attribute_may_be_missing_and_an_unknown_one_is_named() {
    let optional = spec(AttributeType::Text, false);
    assert!(optional.check(&BTreeMap::new()).is_ok());
    assert_eq!(
        optional
            .check(&one("colour", Attribute::Text("red".to_owned())))
            .unwrap_err(),
        "attributes.colour is not an attribute of deploy events"
    );
    assert_eq!(
        spec(AttributeType::Text, true)
            .check(&BTreeMap::new())
            .unwrap_err(),
        "attributes.region is missing; deploy events carry it"
    );
}

#[test]
fn an_attribute_or_link_name_is_readable_by_a_template() {
    for name in ["a", "region", "build_2"] {
        assert!(readable(name), "{name}");
    }
    for name in ["", "2a", "_a", "Region", "a-b", "a b"] {
        assert!(!readable(name), "{name}");
    }
}

#[test]
fn the_kinds_a_config_knows_are_herald_s_and_those_it_declares_in_order() {
    let deploy = spec(AttributeType::Text, true);
    let kinds = KindSpecs::default().with(deploy.clone());
    assert_eq!(kinds.get(&deploy.kind), Some(&deploy));
    assert_eq!(
        kinds.get(&Kind::RELEASE).map(|spec| spec.versioned),
        Some(true)
    );
    assert_eq!(KindSpecs::default().get(&deploy.kind), None);
    let names: Vec<&str> = kinds.iter().map(|spec| spec.kind.as_str()).collect();
    assert_eq!(
        names,
        [
            "release",
            "package-version",
            "feed-entry",
            "message",
            "run",
            "alert",
            "pull-request",
            "deploy"
        ]
    );
}

#[test]
fn a_kind_s_links_are_known_by_its_kind_and_none_for_an_unknown_one() {
    let kinds = KindSpecs::default();
    assert_eq!(
        kinds.links(&Kind::RELEASE),
        [LinkSpec::named("release"), LinkSpec::named("changelog")]
    );
    assert_eq!(kinds.links(&Kind::new("deploy").unwrap()), []);
}

#[test]
fn a_pull_request_names_its_number_and_what_it_did() {
    let shipped = KindSpec::shipped();
    let pull = &shipped[6];
    assert_eq!(pull.kind, Kind::PULL_REQUEST);
    assert!(!pull.private);
    assert_eq!(pull.links, [LinkSpec::named("pull")]);
    let mut attributes = BTreeMap::from([
        (
            crate::SIGNAL.to_owned(),
            Attribute::Text("pull-request".to_owned()),
        ),
        (
            crate::ACTION.to_owned(),
            Attribute::Text("opened".to_owned()),
        ),
        (crate::NUMBER.to_owned(), Attribute::Number(12.into())),
        (
            crate::AUTHOR.to_owned(),
            Attribute::Text("dependabot[bot]".to_owned()),
        ),
        (crate::BOT.to_owned(), Attribute::Flag(true)),
        (crate::BASE.to_owned(), Attribute::Text("main".to_owned())),
        (crate::HEAD.to_owned(), Attribute::Text("bump".to_owned())),
        (crate::DRAFT.to_owned(), Attribute::Flag(false)),
        (
            crate::MERGED_BY.to_owned(),
            Attribute::Text("octocat".to_owned()),
        ),
        (
            crate::LABELS.to_owned(),
            Attribute::Text("deps, ci".to_owned()),
        ),
    ]);
    assert!(pull.check(&attributes).is_ok());
    attributes.insert(crate::BOT.to_owned(), Attribute::Text("yes".to_owned()));
    assert!(pull.check(&attributes).is_err());
    attributes.remove(crate::BOT);
    for required in [crate::ACTION, crate::NUMBER] {
        let mut missing = attributes.clone();
        missing.remove(required);
        assert!(pull.check(&missing).is_err(), "{required}");
    }
}
