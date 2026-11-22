use herald_core::{CodeownersRef, MOST_CODEOWNERS_READ, Zoning};
use herald_kinds::Codeowning;

use super::*;

const OWNER: &str = r#"
owner = "acme"
areas = "codeowners"
codeowners_ref = "default"

[repositories."plugin-*"]
codeowners = ["docs/OWNERS"]
codeowners_ref = "base"

[repositories."site"]
areas = { web = ["web/**"] }
"#;

fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

#[test]
fn a_repository_reads_codeowners_as_its_narrowest_override_says_or_its_forge_does() {
    let options: SourceOptions = toml::from_str(OWNER).unwrap();
    assert_eq!(options.check(Forge::GitLab), Ok(()));
    assert!(options.codeowned());
    assert_eq!(options.areas_of("app"), Some(&Zoning::Codeowners));
    assert_eq!(
        options.codeowning_of(Forge::GitLab, "app"),
        Some(Codeowning {
            paths: paths(&["CODEOWNERS", "docs/CODEOWNERS", ".gitlab/CODEOWNERS"]),
            reference: CodeownersRef::Default,
            most: MOST_CODEOWNERS_READ,
            most_areas: 200,
        })
    );
    assert_eq!(
        options.codeowning_of(Forge::GitLab, "plugin-sonarr"),
        Some(Codeowning {
            paths: paths(&["docs/OWNERS"]),
            reference: CodeownersRef::Base,
            most: MOST_CODEOWNERS_READ,
            most_areas: 200,
        })
    );
    assert_eq!(options.codeowning_of(Forge::GitLab, "site"), None);
    assert_eq!(options.area_names(), BTreeSet::from(["web".to_owned()]));
    let source = crate::ForgeSource {
        forge: Forge::GitLab,
        options: options.clone(),
        names: None,
    };
    let source: &dyn herald_kinds::SourceConfig = &source;
    assert!(source.codeowned());
    assert_eq!(
        source.codeowning("acme/plugin-x"),
        options.codeowning_of(Forge::GitLab, "plugin-x")
    );
    assert_eq!(
        source.codeowners_notes(),
        options.codeowners_notes(Forge::GitLab)
    );
    let single: SourceOptions = toml::from_str(
        "repository = \"acme/site\"\nareas = \"codeowners\"\nmost_codeowners = \"1MiB\"\ncodeowners = [\"OWNERS\"]\n",
    )
    .unwrap();
    assert_eq!(single.check(Forge::GitHub), Ok(()));
    assert_eq!(
        single.codeowning_of(Forge::GitHub, "site"),
        Some(Codeowning {
            paths: paths(&["OWNERS"]),
            reference: CodeownersRef::Base,
            most: 1 << 20,
            most_areas: 200,
        })
    );
    assert!(!owner("acme").codeowned());
    let plain = crate::ForgeSource {
        forge: Forge::GitHub,
        options: SourceOptions {
            repository: Some("acme/site".to_owned()),
            ..SourceOptions::default()
        },
        names: None,
    };
    let plain: &dyn herald_kinds::SourceConfig = &plain;
    assert!(!plain.codeowned());
    assert_eq!(plain.codeowning("acme/site"), None);
}

#[test]
fn each_forge_looks_where_and_at_the_branch_it_applies_codeowners_itself() {
    assert_eq!(
        Forge::GitHub.codeowners(),
        [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"]
    );
    assert_eq!(
        Forge::GitLab.codeowners(),
        ["CODEOWNERS", "docs/CODEOWNERS", ".gitlab/CODEOWNERS"]
    );
    assert_eq!(
        Forge::Forgejo.codeowners(),
        [
            "CODEOWNERS",
            "docs/CODEOWNERS",
            ".gitea/CODEOWNERS",
            ".forgejo/CODEOWNERS"
        ]
    );
    assert_eq!(Forge::GitHub.codeowners_ref(), CodeownersRef::Base);
    assert_eq!(Forge::GitLab.codeowners_ref(), CodeownersRef::Base);
    assert_eq!(Forge::Forgejo.codeowners_ref(), CodeownersRef::Default);
}

fn refused(toml: &str) -> String {
    let options: SourceOptions = toml::from_str(toml).unwrap();
    options.check(Forge::GitHub).unwrap_err()
}

#[test]
fn codeowners_keys_are_refused_where_they_break_a_bound_or_read_nothing() {
    let base = "repository = \"acme/site\"\nareas = \"codeowners\"\n";
    assert_eq!(
        refused(&format!("{base}most_codeowners = \"4MiB\"\n")),
        "most_codeowners is at most 3 MiB, GitHub's own limit; lower it"
    );
    let three = format!("{base}most_codeowners = \"3MiB\"\n");
    assert_eq!(
        toml::from_str::<SourceOptions>(&three)
            .unwrap()
            .check(Forge::GitHub),
        Ok(())
    );
    assert_eq!(
        refused(&format!("{base}codeowners = []\n")),
        "codeowners names 0 places, and 1 to 8 are read"
    );
    let nine: Vec<String> = (0..=MOST_CODEOWNERS_PATHS)
        .map(|at| format!("\"d{at}/C\""))
        .collect();
    assert_eq!(
        refused(&format!("{base}codeowners = [{}]\n", nine.join(", "))),
        "codeowners names 9 places, and 1 to 8 are read"
    );
    let eight = &nine[..MOST_CODEOWNERS_PATHS];
    let fits = format!("{base}codeowners = [{}]\n", eight.join(", "));
    assert_eq!(
        toml::from_str::<SourceOptions>(&fits)
            .unwrap()
            .check(Forge::GitHub),
        Ok(())
    );
    for path in [
        "/CODEOWNERS",
        "docs//CODEOWNERS",
        "../CODEOWNERS",
        "./CODEOWNERS",
        "docs/",
        "a\\u0007b",
    ] {
        let said = refused(&format!("{base}codeowners = [\"{path}\"]\n"));
        assert!(said.starts_with("codeowners: \""), "{path}: {said}");
        assert!(
            said.ends_with("with no leading /, no . or .. and no empty part"),
            "{path}: {said}"
        );
    }
    let longest = "a".repeat(MOST_CODEOWNERS_PATH);
    let fits = format!("{base}codeowners = [\"{longest}\"]\n");
    assert_eq!(
        toml::from_str::<SourceOptions>(&fits)
            .unwrap()
            .check(Forge::GitHub),
        Ok(())
    );
    let longer = "a".repeat(MOST_CODEOWNERS_PATH + 1);
    assert_eq!(
        refused(&format!("{base}codeowners = [\"{longer}\"]\n")),
        format!("codeowners: \"{longer}\" is longer than 256 characters; shorten it")
    );
    let owner =
        "owner = \"acme\"\nareas = \"codeowners\"\n[repositories.\"x-*\"]\ncodeowners = [\"..\"]\n";
    assert!(
        refused(owner).starts_with("repositories.\"x-*\".codeowners: \"..\" is no path"),
        "{}",
        refused(owner)
    );
    let unread = "codeowners, codeowners_ref and most_codeowners are read only where areas = \"codeowners\"; set it, or remove them";
    for key in [
        "codeowners = [\"C\"]",
        "codeowners_ref = \"base\"",
        "most_codeowners = \"1MiB\"",
    ] {
        assert_eq!(
            refused(&format!("repository = \"acme/site\"\n{key}\n")),
            unread,
            "{key}"
        );
    }
    for key in ["codeowners = [\"C\"]", "codeowners_ref = \"base\""] {
        let overridden = format!("owner = \"acme\"\n[repositories.\"x-*\"]\n{key}\n");
        assert_eq!(refused(&overridden), unread, "{key}");
    }
    let tied = "owner = \"acme\"\nareas = \"codeowners\"\n[repositories.\"a-*\"]\ncodeowners_ref = \"base\"\n[repositories.\"*-b\"]\ncodeowners_ref = \"default\"\n";
    assert!(
        refused(tied).contains("both set codeowners_ref"),
        "{}",
        refused(tied)
    );
}

#[test]
fn doctor_names_where_and_at_which_branch_codeowners_is_read_and_how_gitlab_names_are_told() {
    let notes = |forge: Forge, toml: &str| {
        toml::from_str::<SourceOptions>(toml)
            .unwrap()
            .codeowners_notes(forge)
    };
    assert_eq!(
        notes(Forge::GitHub, "repository = \"acme/site\"\n"),
        Vec::<String>::new()
    );
    let github = notes(
        Forge::GitHub,
        "repository = \"acme/site\"\nareas = \"codeowners\"\n",
    );
    assert_eq!(github.len(), 2);
    assert!(
        github[0].starts_with("reads areas from each repository's CODEOWNERS file at the branch each pull request merges into, looking at .github/CODEOWNERS, CODEOWNERS, docs/CODEOWNERS in order and reading the first found, at most 131072 bytes of it"),
        "{}",
        github[0]
    );
    assert!(
        github[0].contains("an email is never an area"),
        "{}",
        github[0]
    );
    assert!(
        github[0].contains("taken as written, unchecked"),
        "{}",
        github[0]
    );
    let gitlab = notes(
        Forge::GitLab,
        "repository = \"acme/site\"\nareas = \"codeowners\"\ncodeowners_ref = \"default\"\n",
    );
    assert_eq!(gitlab.len(), 3);
    assert!(
        gitlab[0].contains("at the repository's default branch"),
        "{}",
        gitlab[0]
    );
    assert!(
        gitlab[2].contains("at most 50 names a file"),
        "{}",
        gitlab[2]
    );
    let forgejo = notes(
        Forge::Forgejo,
        "repository = \"acme/site\"\nareas = \"codeowners\"\n",
    );
    assert_eq!(forgejo.len(), 3);
    assert!(
        forgejo[2].contains("on Gitea, set codeowners_ref = \"base\""),
        "{}",
        forgejo[2]
    );
    let based = notes(
        Forge::Forgejo,
        "repository = \"acme/site\"\nareas = \"codeowners\"\ncodeowners_ref = \"base\"\n",
    );
    assert_eq!(based.len(), 2);
    assert!(
        based[0].contains("at the branch each pull request merges into"),
        "{}",
        based[0]
    );
    let owner = notes(Forge::GitHub, OWNER);
    assert_eq!(owner.len(), 3);
    assert!(
        owner[2].starts_with("an override under repositories"),
        "{}",
        owner[2]
    );
    for key in [
        "codeowners = [\"C\"]",
        "codeowners_ref = \"base\"",
        "areas = \"codeowners\"",
    ] {
        let one =
            format!("owner = \"acme\"\nareas = \"codeowners\"\n[repositories.\"x-*\"]\n{key}\n");
        assert_eq!(notes(Forge::GitHub, &one).len(), 3, "{key}");
    }
    let overridden_only = "owner = \"acme\"\n[repositories.\"x-*\"]\nareas = \"codeowners\"\n";
    assert_eq!(notes(Forge::GitHub, overridden_only).len(), 3);
}

#[test]
fn most_areas_bounds_each_table_and_file_and_is_refused_out_of_range_or_unread() {
    let table = "repository = \"acme/site\"\nareas = { web = [\"web/**\"], api = [\"api/**\"] }\n";
    let options: SourceOptions = toml::from_str(table).unwrap();
    assert_eq!(options.check(Forge::GitHub), Ok(()));
    assert_eq!(options.most_areas(), 200);
    assert_eq!(
        refused(&format!("{table}most_areas = 1\n")),
        "areas names 2 areas, and the source reads at most 1; name fewer, or raise most_areas, at most 2000"
    );
    let at = toml::from_str::<SourceOptions>(&format!("{table}most_areas = 2\n")).unwrap();
    assert_eq!(at.check(Forge::GitHub), Ok(()));
    let overridden = "owner = \"acme\"\nmost_areas = 1\n[repositories.\"site\"]\nareas = { web = [\"web/**\"], api = [\"api/**\"] }\n";
    assert_eq!(
        refused(overridden),
        "repositories.\"site\".areas names 2 areas, and the source reads at most 1; name fewer, or raise most_areas, at most 2000"
    );
    let base = "repository = \"acme/site\"\nareas = \"codeowners\"\n";
    for out in [0, 2001] {
        assert_eq!(
            refused(&format!("{base}most_areas = {out}\n")),
            "most_areas is 1 to 2000: each area is a group of a board and is matched against every changed path; give one in that range",
            "{out}"
        );
    }
    for bound in [1, 2000] {
        let read =
            toml::from_str::<SourceOptions>(&format!("{base}most_areas = {bound}\n")).unwrap();
        assert_eq!(read.check(Forge::GitHub), Ok(()), "{bound}");
        assert_eq!(
            read.codeowning_of(Forge::GitHub, "acme/site")
                .map(|codeowning| codeowning.most_areas),
            Some(bound)
        );
    }
    assert_eq!(
        refused("repository = \"acme/site\"\nmost_areas = 300\n"),
        "most_areas is read only where areas is set; set it, or remove most_areas"
    );
}

#[test]
fn doctor_names_most_areas_and_the_304_a_kept_unchanged_file_costs() {
    let notes = |forge: Forge, toml: &str| {
        toml::from_str::<SourceOptions>(toml)
            .unwrap()
            .codeowners_notes(forge)
    };
    let github = notes(
        Forge::GitHub,
        "repository = \"acme/site\"\nareas = \"codeowners\"\n",
    );
    assert!(
        github[1].starts_with("reads at most 200 areas a CODEOWNERS file names, refusing a file naming more whole, which doctor names; raise most_areas, at most 2000"),
        "{}",
        github[1]
    );
    assert!(
        github[1].ends_with(
            "so one unchanged since costs a 304 answer and is neither downloaded nor read again"
        ),
        "{}",
        github[1]
    );
    let raised = notes(
        Forge::GitHub,
        "repository = \"acme/site\"\nareas = \"codeowners\"\nmost_areas = 500\n",
    );
    assert!(
        raised[1].starts_with("reads at most 500 areas"),
        "{}",
        raised[1]
    );
}
