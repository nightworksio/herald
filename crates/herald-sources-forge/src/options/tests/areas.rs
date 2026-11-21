use super::*;

const OWNER: &str = r#"
owner = "acme"
visibility = ["public", "private"]

[areas]
web = ["web/**"]
docs = ["*.md"]

[repositories."plugin-*".areas]
plugin = ["src/**"]

[repositories."plugin-sonarr".areas]
arr = ["arr/**"]
"#;

#[test]
fn a_repository_s_areas_are_its_narrowest_override_s_or_the_source_s() {
    let options: SourceOptions = toml::from_str(OWNER).unwrap();
    assert_eq!(options.check(Forge::GitHub), Ok(()));
    let names = |name: &str| -> Vec<String> {
        options
            .areas_of(name)
            .map(|areas| areas.names().map(str::to_owned).collect())
            .unwrap_or_default()
    };
    assert_eq!(names("site"), ["docs", "web"]);
    assert_eq!(names("plugin-radarr"), ["plugin"]);
    assert_eq!(names("plugin-sonarr"), ["arr"]);
    assert_eq!(
        options.area_names(),
        BTreeSet::from(["arr", "docs", "plugin", "web"].map(str::to_owned))
    );
    let source = crate::ForgeSource {
        forge: Forge::GitHub,
        options: options.clone(),
        names: None,
    };
    let source: &dyn herald_kinds::SourceConfig = &source;
    assert_eq!(
        source.areas("acme/plugin-sonarr"),
        options.areas_of("plugin-sonarr")
    );
    assert_eq!(source.area_names(), options.area_names());
    let bare = owner("acme");
    assert_eq!(bare.areas_of("site"), None);
    assert_eq!(bare.area_names(), BTreeSet::new());
}

#[test]
fn an_areas_table_is_refused_where_it_breaks_a_bound_and_two_as_narrow_overrides_tie() {
    let refused =
        toml::from_str::<SourceOptions>("repository = \"acme/site\"\n[areas]\nweb = []\n")
            .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("the area \"web\" names no path; give it a glob such as \"web/**\""),
        "{refused}"
    );
    let tied: SourceOptions = toml::from_str(
        "owner = \"acme\"\n[repositories.\"a*\".areas]\nx = [\"x/**\"]\n[repositories.\"*b\".areas]\ny = [\"y/**\"]\n",
    )
    .unwrap();
    assert_eq!(
        tied.check(Forge::GitHub),
        Err("repositories \"*b\" and \"a*\" both set areas, are as narrow as each other and could both name one repository; make one narrower".to_owned())
    );
}
