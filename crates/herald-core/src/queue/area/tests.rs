use super::*;

fn table(areas: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    areas
        .iter()
        .map(|(name, globs)| {
            (
                (*name).to_owned(),
                globs.iter().map(|glob| (*glob).to_owned()).collect(),
            )
        })
        .collect()
}

fn named(areas: Vec<Area>) -> Vec<String> {
    assert!(areas.iter().all(|area| area.kind == AreaKind::Paths));
    areas.into_iter().map(|area| area.name).collect()
}

fn changed(paths: &[&str], whole: bool) -> Changed {
    Changed {
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        whole,
    }
}

#[test]
fn a_pull_request_falls_in_each_area_one_of_its_paths_matches_in_name_order() {
    let areas = Areas::of(table(&[
        ("web", &["web/**", "packages/ui/**"]),
        ("docs", &["*.md"]),
        ("api", &["api/**"]),
    ]))
    .unwrap();
    assert_eq!(areas.names().collect::<Vec<_>>(), ["api", "docs", "web"]);
    let read = changed(
        &["packages/UI/button.ts", "docs/guide.md", "Cargo.lock"],
        false,
    );
    assert_eq!(named(areas.of_changed(Some(&read))), ["docs", "web"]);
    assert_eq!(named(areas.of_changed(None)), Vec::<String>::new());
    let none = Vec::<String>::new();
    assert_eq!(
        named(areas.of_changed(Some(&changed(&["Cargo.lock"], true)))),
        none
    );
    assert_eq!(named(Areas::default().of_changed(Some(&read))), none);
}

#[test]
fn a_config_s_table_is_read_through_the_same_checks() {
    let read: Areas = serde_json::from_value(serde_json::json!({ "web": ["web/**"] })).unwrap();
    assert_eq!(read, Areas::of(table(&[("web", &["web/**"])])).unwrap());
    let refused = serde_json::from_value::<Areas>(serde_json::json!({ "web": [] })).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "the area \"web\" names no path; give it a glob such as \"web/**\""
    );
}

#[test]
fn a_path_longer_than_the_most_matched_falls_in_no_area() {
    let areas = Areas::of(table(&[("all", &["*"])])).unwrap();
    let longest = "a".repeat(MOST_AREA_PATH);
    let longer = "a".repeat(MOST_AREA_PATH + 1);
    assert_eq!(
        named(areas.of_changed(Some(&changed(&[&longest], true)))),
        ["all"]
    );
    assert_eq!(
        named(areas.of_changed(Some(&changed(&[&longer], true)))),
        Vec::<String>::new()
    );
    let wide = "é".repeat(MOST_AREA_PATH);
    assert_eq!(
        named(areas.of_changed(Some(&changed(&[&wide], true)))),
        ["all"]
    );
}

#[test]
fn an_area_name_is_written_unpadded_short_and_without_control_characters() {
    let one = |name: &str| Areas::of(table(&[(name, &["x/**"])]));
    assert!(one("@org/web team").is_ok());
    assert!(one(&"é".repeat(MOST_AREA_NAME)).is_ok());
    for blank in ["", " ", " web", "web "] {
        assert_eq!(
            one(blank).unwrap_err(),
            format!("\"{blank}\" is no area name; give one without spaces around it")
        );
    }
    let long = "w".repeat(MOST_AREA_NAME + 1);
    assert_eq!(
        one(&long).unwrap_err(),
        format!("\"{long}\" is longer than {MOST_AREA_NAME} characters; shorten it")
    );
    assert_eq!(
        one("we\nb").unwrap_err(),
        "\"we\\nb\" holds a control character; remove it"
    );
}

#[test]
fn an_area_names_at_least_one_readable_glob_of_bounded_length() {
    assert_eq!(
        Areas::of(table(&[("web", &[])])).unwrap_err(),
        "the area \"web\" names no path; give it a glob such as \"web/**\""
    );
    assert_eq!(
        Areas::of(table(&[("web", &["web/[a"])])).unwrap_err(),
        "the area \"web\": \"web/[a\" opens a [ it never closes with a character in it; close it, or write [[] for a [ itself"
    );
    let longest = "é".repeat(MOST_AREA_GLOB);
    assert!(Areas::of(table(&[("web", &[&longest])])).is_ok());
    let longer = "a".repeat(MOST_AREA_GLOB + 1);
    assert_eq!(
        Areas::of(table(&[("web", &[&longer])])).unwrap_err(),
        format!("the area \"web\" has a glob longer than {MOST_AREA_GLOB} characters; shorten it")
    );
}

#[test]
fn a_table_names_at_most_the_most_areas_and_globs() {
    let areas = |count: usize, globs: usize| -> BTreeMap<String, Vec<String>> {
        (0..count)
            .map(|at| (format!("a{at}"), vec!["x/**".to_owned(); globs]))
            .collect()
    };
    assert!(Areas::of(areas(MOST_AREA_GLOBS, 1)).is_ok());
    assert_eq!(
        Areas::of(areas(MOST_AREAS, 1)).unwrap_err(),
        format!(
            "the areas hold {MOST_AREAS} globs, and at most {MOST_AREA_GLOBS} are read; give fewer"
        )
    );
    assert_eq!(
        Areas::of(areas(MOST_AREAS + 1, 1)).unwrap_err(),
        format!(
            "{} areas are named, and at most {MOST_AREAS} are read; name fewer",
            MOST_AREAS + 1
        )
    );
    assert!(Areas::of(areas(MOST_AREA_GLOBS / 10, 10)).is_ok());
    assert_eq!(
        Areas::of(areas(MOST_AREA_GLOBS / 10 + 1, 10)).unwrap_err(),
        format!(
            "the areas hold {} globs, and at most {MOST_AREA_GLOBS} are read; give fewer",
            MOST_AREA_GLOBS + 10
        )
    );
}

#[test]
fn a_source_s_areas_are_a_table_of_globs_or_codeowners() {
    let globbed: Zoning = serde_json::from_value(serde_json::json!({ "web": ["web/**"] })).unwrap();
    assert_eq!(
        globbed,
        Zoning::Paths(Areas::of(table(&[("web", &["web/**"])])).unwrap())
    );
    assert_eq!(globbed.names().collect::<Vec<_>>(), ["web"]);
    let read: Zoning = serde_json::from_value(serde_json::json!("codeowners")).unwrap();
    assert_eq!(read, Zoning::Codeowners);
    assert_eq!(read.names().count(), 0);
    let refused = serde_json::from_value::<Zoning>(serde_json::json!("owners")).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "\"owners\" names no way to read areas; give a table of path globs, or \"codeowners\""
    );
    let empty = serde_json::from_value::<Zoning>(serde_json::json!({ "web": [] })).unwrap_err();
    assert_eq!(
        empty.to_string(),
        "the area \"web\" names no path; give it a glob such as \"web/**\""
    );
}

#[test]
fn each_kind_of_area_is_written_as_its_json_marks_it() {
    let kinds = [
        (AreaKind::Paths, "paths"),
        (AreaKind::Team, "team"),
        (AreaKind::Group, "group"),
        (AreaKind::Person, "person"),
        (AreaKind::Role, "role"),
        (AreaKind::Section, "section"),
        (AreaKind::Name, "name"),
    ];
    for (kind, name) in kinds {
        assert_eq!(kind.name(), name);
        assert_eq!(serde_json::to_value(kind).unwrap(), serde_json::json!(name));
    }
}
