use super::*;

fn owned(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn words_split_at_unescaped_spaces_and_tabs() {
    assert_eq!(words("a\\ b \t @x  ", Dialect::GitHub), ["a\\ b", "@x"]);
    assert_eq!(words("a\\", Dialect::GitLab), ["a\\"]);
    assert_eq!(words("a\\ b @x # c", Dialect::Forgejo), ["a b", "@x"]);
    assert_eq!(words("\\#x @y", Dialect::Forgejo), ["#x", "@y"]);
    assert_eq!(words("a\\", Dialect::Forgejo), ["a"]);
    assert_eq!(words("a#b @x", Dialect::GitHub), ["a#b", "@x"]);
    assert_eq!(words("#", Dialect::Forgejo), Vec::<String>::new());
}

#[test]
fn github_reads_logins_teams_and_emails() {
    let github = |word: &str| owner(word, Dialect::GitHub);
    assert_eq!(github("@u"), Some(CodeOwner::Person("@u".to_owned())));
    assert_eq!(github("@o/t"), Some(CodeOwner::Team("@o/t".to_owned())));
    assert_eq!(github("a@b.c"), Some(CodeOwner::Email("a@b.c".to_owned())));
    for word in [
        "@o/t/x", "@", "u", "@a@b", "@/t", "@o/", "a@", "a@b@c", "a/b@c",
    ] {
        assert_eq!(github(word), None, "{word}");
    }
    assert_eq!(
        github(&format!("@{}", "u".repeat(127))).map(|owner| owner.written().len()),
        Some(128)
    );
    assert_eq!(github(&format!("@{}", "u".repeat(128))), None);
    assert_eq!(github("@u\u{7}"), None);
}

#[test]
fn gitlab_reads_roles_groups_names_and_emails() {
    let gitlab = |word| owner(word, Dialect::GitLab);
    assert_eq!(
        gitlab("@@developer"),
        Some(CodeOwner::Role("@@developer".to_owned()))
    );
    assert_eq!(gitlab("@g/s/t"), Some(CodeOwner::Team("@g/s/t".to_owned())));
    assert_eq!(gitlab("@name"), Some(CodeOwner::Either("@name".to_owned())));
    assert_eq!(gitlab("x@y"), Some(CodeOwner::Email("x@y".to_owned())));
    for word in ["@@", "@@a/b", "@@a@b", "plain", "@g//s", "@", "@a@b", "#x"] {
        assert_eq!(gitlab(word), None, "{word}");
    }
}

#[test]
fn forgejo_reads_logins_and_teams_with_or_without_an_at() {
    let forgejo = |word| owner(word, Dialect::Forgejo);
    assert_eq!(forgejo("user"), Some(CodeOwner::Person("@user".to_owned())));
    assert_eq!(
        forgejo("@user"),
        Some(CodeOwner::Person("@user".to_owned()))
    );
    assert_eq!(
        forgejo("org/team"),
        Some(CodeOwner::Team("@org/team".to_owned()))
    );
    for word in ["a/b/c", "@", "a@b", "/t", "o/"] {
        assert_eq!(forgejo(word), None, "{word}");
    }
}

#[test]
fn an_owner_is_written_as_its_forge_writes_it() {
    for owner in [
        CodeOwner::Team("@o/t".to_owned()),
        CodeOwner::Person("@o/t".to_owned()),
        CodeOwner::Either("@o/t".to_owned()),
        CodeOwner::Email("@o/t".to_owned()),
        CodeOwner::Role("@o/t".to_owned()),
    ] {
        assert_eq!(owner.written(), "@o/t");
    }
}

#[test]
fn github_skips_a_line_with_an_owner_it_does_not_read_and_ends_at_a_comment() {
    assert_eq!(
        owners(&owned(&["@a", "#", "comment", "x"]), Dialect::GitHub).unwrap(),
        [CodeOwner::Person("@a".to_owned())]
    );
    assert_eq!(
        owners(&owned(&["@a", "plain"]), Dialect::GitHub).unwrap_err(),
        "\"plain\" is no owner GitHub reads: write @login, @org/team or an email address"
    );
    assert_eq!(
        owners(&owned(&["@a", "plain", "#x"]), Dialect::GitLab).unwrap(),
        [CodeOwner::Either("@a".to_owned())]
    );
    assert_eq!(
        owners(&owned(&["a/b/c", "u"]), Dialect::Forgejo).unwrap(),
        [CodeOwner::Person("@u".to_owned())]
    );
}

#[test]
fn a_line_names_at_most_its_owners() {
    let most: Vec<String> = (0..MOST_LINE_OWNERS).map(|at| format!("@u{at}")).collect();
    assert_eq!(
        owners(&most, Dialect::GitHub).unwrap().len(),
        MOST_LINE_OWNERS
    );
    let past: Vec<String> = (0..=MOST_LINE_OWNERS).map(|at| format!("@u{at}")).collect();
    for dialect in [Dialect::GitHub, Dialect::GitLab, Dialect::Forgejo] {
        assert_eq!(
            owners(&past, dialect).unwrap_err(),
            format!(
                "it names {} owners, and at most {MOST_LINE_OWNERS} are read",
                MOST_LINE_OWNERS + 1
            )
        );
    }
    let mut commented = most.clone();
    commented.extend(owned(&["#", "more"]));
    assert_eq!(
        owners(&commented, Dialect::GitHub).unwrap().len(),
        MOST_LINE_OWNERS
    );
}

fn defaults(line: &str) -> Vec<String> {
    heading(line)
        .unwrap()
        .unwrap()
        .owners
        .iter()
        .map(|owner| owner.written().to_owned())
        .collect()
}

#[test]
fn a_gitlab_heading_names_its_section_and_default_owners() {
    assert_eq!(
        heading("[Docs]"),
        Some(Ok(Heading {
            name: "Docs".to_owned(),
            owners: Vec::new()
        }))
    );
    assert_eq!(
        heading("^[ Docs ][2] @a @g/x"),
        Some(Ok(Heading {
            name: " Docs ".to_owned(),
            owners: vec![
                CodeOwner::Either("@a".to_owned()),
                CodeOwner::Team("@g/x".to_owned())
            ]
        }))
    );
    assert_eq!(heading("docs/"), None);
    assert_eq!(heading("^docs"), None);
}

#[test]
fn a_gitlab_heading_takes_only_digits_and_spaces_as_its_approvals() {
    assert_eq!(defaults("^[Name][2] @a"), ["@a"]);
    assert_eq!(defaults("[Name][12]@a"), ["@a"]);
    assert_eq!(defaults("[Name][ 1] @a"), ["@a"]);
    assert_eq!(defaults("[Name][] @a"), ["@a"]);
    assert_eq!(defaults("[Name][@owner] @a path"), Vec::<String>::new());
    assert_eq!(defaults("[Name][x] @a"), Vec::<String>::new());
    assert_eq!(defaults("[Name][2x] @a"), Vec::<String>::new());
    assert_eq!(defaults("[Name]] @a"), Vec::<String>::new());
    assert_eq!(defaults("[Name] @a # @b"), ["@a"]);
    assert_eq!(defaults("[Name] @a, @b"), ["@a"]);
    assert_eq!(defaults("[Name] @a.b-c_d/e"), ["@a.b-c_d/e"]);
    assert_eq!(defaults("[Name]\t@a"), ["@a"]);
    for line in ["[Name][@owner] path", "[Name][x]", "[Name]]", "[]", "[ ]"] {
        assert!(heading(line).unwrap().is_ok(), "{line}");
    }
    assert_eq!(heading("[a\u{7}b]").unwrap().unwrap().name, "a\u{7}b");
}

#[test]
fn a_gitlab_heading_that_never_closes_or_names_too_many_owners_is_refused() {
    assert_eq!(
        heading("[Docs").unwrap().unwrap_err(),
        "its section heading never closes with \"]\""
    );
    let crowded = format!(
        "[Docs] {}",
        (0..=MOST_LINE_OWNERS)
            .map(|at| format!("@u{at}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert!(heading(&crowded).unwrap().is_err());
}
