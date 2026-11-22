use std::fmt::Write as _;

use super::*;
use crate::MOST_AREAS_READ;

fn changed(paths: &[&str]) -> Changed {
    Changed {
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        whole: true,
    }
}

fn parsed(dialect: Dialect, text: &str) -> Codeowners {
    Codeowners::read(dialect, text, MOST_AREAS_READ)
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
    let read = parsed(Dialect::GitHub, GITHUB);
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
    let read = parsed(Dialect::GitHub, "* @a\nx[y] @b\n/x @c plain\n\\#x @d\n");
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
    let read = parsed(Dialect::GitLab, GITLAB);
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
    let read = parsed(
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
    let read = parsed(
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
fn a_gitlab_heading_named_codeowners_goes_on_with_the_default_section() {
    let read = parsed(
        Dialect::GitLab,
        "* @g\n[Docs]\n* @d\n[CodeOwners]\n* @h\n[ ]\n* @s\n[Name][@owner] @x\n* \n",
    );
    assert_eq!(owners(&read, &["x"]), ["@d", "@h", "@s"]);
    assert_eq!(sections(&read, &["x"]), ["Docs", "Name"]);
    assert_eq!(skipped(&read), []);
    let named = parsed(Dialect::GitLab, "[Name][@owner] @x\nx\n");
    assert_eq!(owners(&named, &["x"]), Vec::<String>::new());
    assert_eq!(sections(&named, &["x"]), ["Name"]);
}

#[test]
fn gitlab_reads_entries_after_a_broken_heading_as_the_previous_sections() {
    let read = parsed(
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
    let bare = parsed(Dialect::GitLab, "[Docs]\ndocs/\n");
    assert_eq!(owners(&bare, &["docs/a"]), Vec::<String>::new());
    assert_eq!(sections(&bare, &["docs/a"]), ["Docs"]);
}

#[test]
fn on_forgejo_every_matching_line_applies_and_a_negative_one_to_the_rest() {
    let read = parsed(
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
    let bare = parsed(Dialect::Forgejo, " \\\n");
    assert_eq!(skipped(&bare), [(1, "it names no pattern")]);
}

#[test]
fn a_file_past_its_bounds_is_refused_whole() {
    let mut text = "* @a\n".to_owned();
    text.push_str(&"#".repeat(MOST_CODEOWNERS - text.len()));
    let read = parsed(Dialect::GitHub, &text);
    assert_eq!(read.refused(), None);
    assert_eq!(owners(&read, &["x"]), ["@a"]);
    text.push('#');
    let read = parsed(Dialect::GitHub, &text);
    assert_eq!(
        read.refused(),
        Some("it is longer than 3145728 bytes, GitHub's own limit")
    );
    assert_eq!(owners(&read, &["x"]), Vec::<String>::new());

    let mut lines = "* @a\n".repeat(MOST_CODEOWNERS_LINES);
    let read = parsed(Dialect::GitHub, &lines);
    assert_eq!(read.refused(), None);
    assert_eq!(owners(&read, &["x"]), ["@a"]);
    lines.push_str("* @b\n");
    let read = parsed(Dialect::GitHub, &lines);
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
    let read = parsed(Dialect::GitHub, &format!("{most} @a\n{past} @b\n"));
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
    let few = parsed(Dialect::Forgejo, &line.repeat(2));
    assert_eq!(few.refused(), None);
    assert_eq!(owners(&few, &["ab"]), ["@a"]);
    let many = parsed(Dialect::Forgejo, &format!("{}! @b\n", line.repeat(400)));
    assert_eq!(
        many.refused(),
        Some("its patterns compile to more than 16777216 bytes together")
    );
    assert_eq!(owners(&many, &["ab", "x"]), Vec::<String>::new());
    assert_eq!(skipped(&many), []);
}

#[test]
fn a_path_past_its_most_characters_and_unread_paths_fall_to_no_one() {
    let read = parsed(Dialect::GitHub, "* @a\n");
    assert_eq!(owners(&read, &[&"x".repeat(MOST_AREA_PATH)]), ["@a"]);
    assert_eq!(
        owners(&read, &[&"x".repeat(MOST_AREA_PATH + 1)]),
        Vec::<String>::new()
    );
    assert_eq!(read.owning(None), Owned::default());
    let empty = parsed(Dialect::GitHub, "# nothing\n\n");
    assert_eq!(empty.owning(Some(&changed(&["x"]))), Owned::default());
    assert_eq!(empty.refused(), None);
}

fn kinds(
    read: &Codeowners,
    paths: &[&str],
    resolved: &[(&str, AreaKind)],
) -> Vec<(String, AreaKind)> {
    let owned = read.owning(Some(&changed(paths)));
    read.areas(&owned, |name| {
        resolved
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, kind)| *kind)
    })
    .into_iter()
    .map(|area| (area.name, area.kind))
    .collect()
}

fn area(name: &str, kind: AreaKind) -> (String, AreaKind) {
    (name.to_owned(), kind)
}

#[test]
fn teams_people_roles_and_sections_are_areas_and_an_email_never_is() {
    let github = parsed(Dialect::GitHub, "* @org/web @alice docs@example.com\n");
    assert_eq!(
        kinds(&github, &["a.rs"], &[]),
        [
            area("@alice", AreaKind::Person),
            area("@org/web", AreaKind::Team)
        ]
    );
    let gitlab = parsed(
        Dialect::GitLab,
        "[Docs] @@maintainer\n*.md @group/sub @known @unknown @asked me@example.com\n",
    );
    assert_eq!(
        kinds(
            &gitlab,
            &["guide.md"],
            &[("@known", AreaKind::Person), ("@asked", AreaKind::Group)]
        ),
        [
            area("@asked", AreaKind::Group),
            area("@group/sub", AreaKind::Group),
            area("@known", AreaKind::Person),
            area("@unknown", AreaKind::Name),
            area("Docs", AreaKind::Section)
        ]
    );
    assert_eq!(gitlab.undecided(), ["@known", "@unknown", "@asked"]);
    assert_eq!(github.undecided(), Vec::<&str>::new());
    let forgejo = parsed(Dialect::Forgejo, ".*\\.md @org/docs @bob\n");
    assert_eq!(
        kinds(&forgejo, &["a.md"], &[]),
        [
            area("@bob", AreaKind::Person),
            area("@org/docs", AreaKind::Team)
        ]
    );
}

#[test]
fn a_name_that_cannot_name_an_area_is_never_looked_up_and_a_name_two_lines_write_is_once() {
    let long = format!("@{}", "a".repeat(crate::queue::MOST_AREA_NAME));
    let gitlab = parsed(
        Dialect::GitLab,
        &format!("*.md @twice {long}\n*.rs @twice @@developer\n"),
    );
    assert_eq!(gitlab.undecided(), ["@twice"]);
    assert_eq!(
        kinds(&gitlab, &["a.md", "b.rs"], &[]),
        [
            area("@@developer", AreaKind::Role),
            area("@twice", AreaKind::Name)
        ]
    );
}

#[test]
fn a_file_longer_than_the_source_reads_is_refused_before_it_is_read() {
    let text = "* @org/web\n";
    let read = Codeowners::within(Dialect::GitHub, text.as_bytes(), (text.len(), 1));
    assert_eq!(read.refused(), None);
    assert_eq!(owners(&read, &["a"]), ["@org/web"]);
    let refused = Codeowners::within(Dialect::GitHub, text.as_bytes(), (text.len() - 1, 1));
    assert_eq!(
        refused.refused(),
        Some(
            "it is longer than 10 bytes, the most the source reads; raise most_codeowners, at most 3145728"
        )
    );
    assert_eq!(owners(&refused, &["a"]), Vec::<String>::new());
    assert_eq!(refused.skipped(), &[] as &[Skipped]);
    assert_eq!(MOST_CODEOWNERS_READ, 131_072);
    let two = "a @x\nb @y\n";
    assert_eq!(
        Codeowners::within(Dialect::GitHub, two.as_bytes(), (two.len(), 1)).refused(),
        Some("it names 2 areas, and the source reads at most 1; raise most_areas, at most 2000")
    );
}

#[test]
fn a_file_naming_more_areas_than_its_source_reads_is_refused() {
    let lines = |count: usize| -> String {
        let mut lines = String::new();
        for at in 0..count {
            let _ = writeln!(lines, "/{at} @person{at} person{at}@example.com");
        }
        lines
    };
    let most = parsed(Dialect::GitHub, &lines(MOST_AREAS_READ));
    assert_eq!(most.refused(), None);
    assert_eq!(owners(&most, &["0"]), ["@person0", "person0@example.com"]);
    let more = parsed(Dialect::GitHub, &lines(MOST_AREAS_READ + 1));
    assert_eq!(
        more.refused(),
        Some(
            "it names 201 areas, and the source reads at most 200; raise most_areas, at most 2000"
        )
    );
    assert_eq!(owners(&more, &["0"]), Vec::<String>::new());
    let raised = Codeowners::read(
        Dialect::GitHub,
        &lines(MOST_AREAS_READ + 1),
        MOST_AREAS_READ + 1,
    );
    assert_eq!(raised.refused(), None);
    let mut sections = String::new();
    for at in 0..MOST_AREAS_READ {
        let _ = writeln!(sections, "[S{at}]\n/{at} @same");
    }
    let sectioned = parsed(Dialect::GitLab, &sections);
    assert_eq!(
        sectioned.refused(),
        Some(
            "it names 201 areas, and the source reads at most 200; raise most_areas, at most 2000"
        )
    );
    let unnamed = format!("[ padded ]\n/x @same\n{}", lines(MOST_AREAS_READ - 1));
    assert_eq!(parsed(Dialect::GitLab, &unnamed).refused(), None);
    assert_eq!((MOST_AREAS_READ, MOST_AREAS), (200, 2000));
}
