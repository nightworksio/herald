use super::*;

fn spec(ty: AttributeType, required: bool) -> KindSpec {
    KindSpec::new(
        Kind::new("deploy").unwrap(),
        "A deploy.",
        false,
        vec!["site".to_owned()],
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
        "attributes.version is missing; a release event carries it"
    );
    let unversioned = spec(AttributeType::Text, false);
    assert!(!unversioned.versioned);
    assert!(!unversioned.attributes.contains_key(VERSION));
    assert_eq!(unversioned.description, "A deploy.");
    assert_eq!(unversioned.links, ["site"]);
}

#[test]
fn herald_ships_four_kinds_and_only_releases_and_package_versions_are_versioned() {
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
            ("message", false)
        ]
    );
    assert_eq!(shipped[2].links, ["entry"]);
    assert_eq!(shipped[3].links, ["release", "changelog"]);
}

#[test]
fn each_type_holds_its_own_attributes_only() {
    let cases = [
        (AttributeType::Text, Attribute::Text("eu".to_owned()), None),
        (
            AttributeType::Text,
            Attribute::Flag(true),
            Some("attributes.region is not text; a deploy event's region is"),
        ),
        (AttributeType::Flag, Attribute::Flag(false), None),
        (
            AttributeType::Flag,
            Attribute::Number(1.into()),
            Some("attributes.region is not a flag; a deploy event's region is"),
        ),
        (AttributeType::Number, Attribute::Number(1.into()), None),
        (
            AttributeType::Number,
            Attribute::Text("1".to_owned()),
            Some("attributes.region is not a number; a deploy event's region is"),
        ),
        (
            AttributeType::Url,
            Attribute::Text("https://example.org".to_owned()),
            None,
        ),
        (
            AttributeType::Url,
            Attribute::Text("ftp://example.org".to_owned()),
            Some("attributes.region is not an https or http address; a deploy event's region is"),
        ),
        (
            AttributeType::Url,
            Attribute::Flag(true),
            Some("attributes.region is not an https or http address; a deploy event's region is"),
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
        "attributes.colour is not an attribute of a deploy event"
    );
    assert_eq!(
        spec(AttributeType::Text, true)
            .check(&BTreeMap::new())
            .unwrap_err(),
        "attributes.region is missing; a deploy event carries it"
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
            "deploy"
        ]
    );
}
