use super::*;

fn changed(paths: &[&str]) -> Changed {
    Changed {
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        whole: true,
    }
}

fn owners(read: &Codeowners, paths: &[&str]) -> Vec<String> {
    let mut written: Vec<String> = read
        .owning(Some(&changed(paths)))
        .owners
        .iter()
        .map(|owner| owner.written().to_owned())
        .collect();
    written.sort();
    written
}

fn sections(read: &Codeowners, paths: &[&str]) -> Vec<String> {
    read.owning(Some(&changed(paths)))
        .sections
        .into_iter()
        .collect()
}

fn skipped(read: &Codeowners) -> Vec<(usize, &str)> {
    read.skipped()
        .iter()
        .map(|skipped| (skipped.line, skipped.why.as_str()))
        .collect()
}

const GITHUB: &str = "\
# The default owners.
*       @global-owner1 @global-owner2
*.js    @js-owner #This is an inline comment.
*.go docs@example.com
*.txt @octo-org/octocats
/apps/ @octocat
/apps/github
!/secret/ @nobody
";

#[test]
fn on_github_the_last_matching_line_wins() {
    let read = Codeowners::read(Dialect::GitHub, GITHUB);
    assert_eq!(owners(&read, &["src/a.js"]), ["@js-owner"]);
    assert_eq!(
        owners(&read, &["README"]),
        ["@global-owner1", "@global-owner2"]
    );
    assert_eq!(owners(&read, &["main.go"]), ["docs@example.com"]);
    assert_eq!(owners(&read, &["apps/x.txt"]), ["@octocat"]);
    assert_eq!(owners(&read, &["apps/github/x.js"]), Vec::<String>::new());
    assert_eq!(
        owners(&read, &["a.js", "b.txt"]),
        ["@js-owner", "@octo-org/octocats"]
    );
    assert_eq!(
        read.owning(Some(&changed(&["a.txt"]))).owners,
        BTreeSet::from([CodeOwner::Team("@octo-org/octocats".to_owned())])
    );
    assert_eq!(sections(&read, &["a.js"]), Vec::<String>::new());
    assert_eq!(
        skipped(&read),
        [(8, "GitHub does not read a pattern starting with \"!\"")]
    );
    assert_eq!(read.refused(), None);
}

#[test]
fn a_skipped_line_matches_nothing_and_an_earlier_line_wins() {
    let read = Codeowners::read(Dialect::GitHub, "* @a\nx[y] @b\n/x @c plain\n\\#x @d\n");
    for path in ["x[y]", "x", "#x"] {
        assert_eq!(owners(&read, &[path]), ["@a"], "{path}");
    }
    assert_eq!(
        skipped(&read),
        [
            (2, "GitHub does not read \"[ ]\" in a pattern"),
            (
                3,
                "\"plain\" is no owner GitHub reads: write @login, @org/team or an email address"
            ),
            (4, "GitHub does not read a pattern starting with \"\\#\""),
        ]
    );
}

const GITLAB: &str = "\
# Required for all files
* @general-approvers

[Documentation] @docs-team
docs/
README.md
*.txt

^[Database][2] @database-team
model/db/
model/db/database-setup.md @docs-team
";

#[test]
fn on_gitlab_every_section_applies_and_its_last_matching_entry_wins() {
    let read = Codeowners::read(Dialect::GitLab, GITLAB);
    assert_eq!(
        owners(&read, &["model/db/CHANGELOG.txt"]),
        ["@database-team", "@docs-team", "@general-approvers"]
    );
    assert_eq!(
        sections(&read, &["model/db/CHANGELOG.txt"]),
        ["Database", "Documentation"]
    );
    assert_eq!(
        owners(&read, &["model/db/database-setup.md"]),
        ["@docs-team", "@general-approvers"]
    );
    assert_eq!(
        sections(&read, &["model/db/database-setup.md"]),
        ["Database"]
    );
    assert_eq!(owners(&read, &["src/main.rs"]), ["@general-approvers"]);
    assert_eq!(sections(&read, &["src/main.rs"]), Vec::<String>::new());
    assert_eq!(skipped(&read), []);
}

#[test]
fn gitlab_combines_sections_by_name_ignoring_case_keeping_the_first_case() {
    let read = Codeowners::read(
        Dialect::GitLab,
        "[Documentation]\nREADME.md @docs\n[Database]\nREADME.md @database\n[DOCUMENTATION]\nREADME.md @docs2\n",
    );
    assert_eq!(owners(&read, &["README.md"]), ["@database", "@docs2"]);
    assert_eq!(
        sections(&read, &["README.md"]),
        ["Database", "Documentation"]
    );
}

#[test]
fn a_gitlab_exemption_takes_a_path_out_of_its_own_section_alone() {
    let read = Codeowners::read(
        Dialect::GitLab,
        "* @g\n[Docs]\n*.md @a\n!docs/x.md\n[Other]\n!y.md\n*.md @b\n",
    );
    assert_eq!(owners(&read, &["docs/x.md"]), ["@b", "@g"]);
    assert_eq!(sections(&read, &["docs/x.md"]), ["Other"]);
    assert_eq!(owners(&read, &["y.md"]), ["@a", "@g"]);
    assert_eq!(sections(&read, &["y.md"]), ["Docs"]);
    assert_eq!(owners(&read, &["z.md"]), ["@a", "@b", "@g"]);
}

#[test]
fn gitlab_reads_entries_after_a_broken_heading_as_the_previous_sections() {
    let read = Codeowners::read(
        Dialect::GitLab,
        "[Docs] @d\ndocs/\n[Broken\nREADME.md\nnone/ plain\npath\\ with\\ spaces/*.md @s\n",
    );
    assert_eq!(owners(&read, &["README.md"]), ["@d"]);
    assert_eq!(sections(&read, &["README.md"]), ["Docs"]);
    assert_eq!(owners(&read, &["none/x"]), ["@d"]);
    assert_eq!(owners(&read, &["path with spaces/a.md"]), ["@s"]);
    assert_eq!(
        skipped(&read),
        [(3, "its section heading never closes with \"]\"")]
    );
    let bare = Codeowners::read(Dialect::GitLab, "[Docs]\ndocs/\n");
    assert_eq!(owners(&bare, &["docs/a"]), Vec::<String>::new());
    assert_eq!(sections(&bare, &["docs/a"]), ["Docs"]);
}

#[test]
fn on_forgejo_every_matching_line_applies_and_a_negative_one_to_the_rest() {
    let read = Codeowners::read(
        Dialect::Forgejo,
        ".*\\\\.go @a # a comment\ndocs/.* org/team\n!README\\\\.md b\n(x @c\nonly\nx a/b/c\nx\\{9999} @d\n",
    );
    assert_eq!(owners(&read, &["x.go"]), ["@a", "@b"]);
    assert_eq!(owners(&read, &["README.md"]), Vec::<String>::new());
    assert_eq!(owners(&read, &["docs/x"]), ["@b", "@org/team"]);
    assert_eq!(owners(&read, &["(x", "only", "x"]), ["@b"]);
    assert_eq!(skipped(&read).len(), 4);
    assert_eq!(skipped(&read)[0].0, 4);
    assert!(
        skipped(&read)[0]
            .1
            .starts_with("its pattern is no regular expression herald reads: "),
        "{:?}",
        skipped(&read)[0]
    );
    assert_eq!(skipped(&read)[1], (5, "it names no owner Forgejo reads"));
    assert_eq!(skipped(&read)[2], (6, "it names no owner Forgejo reads"));
    assert_eq!(
        skipped(&read)[3],
        (7, "its pattern compiles to more than 262144 bytes")
    );
    let bare = Codeowners::read(Dialect::Forgejo, " \\\n");
    assert_eq!(skipped(&bare), [(1, "it names no pattern")]);
}

#[test]
fn a_file_past_its_bounds_is_refused_whole() {
    let mut text = "* @a\n".to_owned();
    text.push_str(&"#".repeat(MOST_CODEOWNERS - text.len()));
    let read = Codeowners::read(Dialect::GitHub, &text);
    assert_eq!(read.refused(), None);
    assert_eq!(owners(&read, &["x"]), ["@a"]);
    text.push('#');
    let read = Codeowners::read(Dialect::GitHub, &text);
    assert_eq!(
        read.refused(),
        Some("it is longer than 3145728 bytes, GitHub's own limit")
    );
    assert_eq!(owners(&read, &["x"]), Vec::<String>::new());

    let mut lines = "* @a\n".repeat(MOST_CODEOWNERS_LINES);
    let read = Codeowners::read(Dialect::GitHub, &lines);
    assert_eq!(read.refused(), None);
    assert_eq!(owners(&read, &["x"]), ["@a"]);
    lines.push_str("* @b\n");
    let read = Codeowners::read(Dialect::GitHub, &lines);
    assert_eq!(
        read.refused(),
        Some("it has 4001 lines, and at most 4000 are read")
    );
    assert_eq!(owners(&read, &["x"]), Vec::<String>::new());
}

#[test]
fn a_pattern_is_read_up_to_its_most_characters() {
    let most = "a".repeat(MOST_CODEOWNERS_PATTERN);
    let past = "b".repeat(MOST_CODEOWNERS_PATTERN + 1);
    let read = Codeowners::read(Dialect::GitHub, &format!("{most} @a\n{past} @b\n"));
    assert_eq!(owners(&read, &[&most]), ["@a"]);
    assert_eq!(owners(&read, &[&past]), Vec::<String>::new());
    assert_eq!(
        skipped(&read),
        [(2, "its pattern is longer than 1024 characters")]
    );
}

#[test]
fn patterns_compiling_past_their_bound_together_are_refused_whole() {
    let line = "\\\\w{2} @a\n";
    let few = Codeowners::read(Dialect::Forgejo, &line.repeat(2));
    assert_eq!(few.refused(), None);
    assert_eq!(owners(&few, &["ab"]), ["@a"]);
    let many = Codeowners::read(Dialect::Forgejo, &format!("{}! @b\n", line.repeat(400)));
    assert_eq!(
        many.refused(),
        Some("its patterns compile to more than 16777216 bytes together")
    );
    assert_eq!(owners(&many, &["ab", "x"]), Vec::<String>::new());
    assert_eq!(skipped(&many), []);
}

#[test]
fn a_path_past_its_most_characters_and_unread_paths_fall_to_no_one() {
    let read = Codeowners::read(Dialect::GitHub, "* @a\n");
    assert_eq!(owners(&read, &[&"x".repeat(MOST_AREA_PATH)]), ["@a"]);
    assert_eq!(
        owners(&read, &[&"x".repeat(MOST_AREA_PATH + 1)]),
        Vec::<String>::new()
    );
    assert_eq!(read.owning(None), Owned::default());
    let empty = Codeowners::read(Dialect::GitHub, "# nothing\n\n");
    assert_eq!(empty.owning(Some(&changed(&["x"]))), Owned::default());
    assert_eq!(empty.refused(), None);
}
