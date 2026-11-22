use regex::Regex;

use super::*;

fn takes(expression: &str, path: &str) -> bool {
    Regex::new(expression).unwrap().is_match(path)
}

fn github_takes(pattern: &str, path: &str) -> bool {
    takes(&github(pattern).unwrap(), path)
}

fn gitlab_takes(pattern: &str, path: &str) -> bool {
    takes(&gitlab(pattern), path)
}

#[test]
fn github_reads_its_documented_examples() {
    for path in ["a", "a/b/c.txt", ".github/x"] {
        assert!(github_takes("*", path), "{path}");
    }
    assert!(github_takes("*.js", "x.js"));
    assert!(github_takes("*.js", "a/b/x.js"));
    assert!(!github_takes("*.js", "x.jsx"));
    assert!(github_takes("/build/logs/", "build/logs/a"));
    assert!(github_takes("/build/logs/", "build/logs/a/b"));
    assert!(!github_takes("/build/logs/", "a/build/logs/x"));
    assert!(!github_takes("/build/logs/", "build/logs"));
    assert!(github_takes("docs/*", "docs/getting-started.md"));
    assert!(!github_takes("docs/*", "docs/build-app/troubleshooting.md"));
    assert!(!github_takes("docs/*", "a/docs/x"));
    assert!(github_takes("apps/", "apps/x"));
    assert!(github_takes("apps/", "a/b/apps/x/y"));
    assert!(!github_takes("apps/", "apps"));
    assert!(github_takes("/docs/", "docs/a/b"));
    for path in ["build/logs/x", "deeply/nested/logs/y", "logs/z", "logs"] {
        assert!(github_takes("**/logs", path), "{path}");
    }
    assert!(github_takes("/apps/github", "apps/github"));
    assert!(github_takes("/apps/github", "apps/github/x"));
    assert!(!github_takes("/apps/github", "apps/githubber"));
    assert!(!github_takes("/apps/github", "x/apps/github"));
}

#[test]
fn github_reads_double_asterisks_first_within_and_last() {
    assert!(github_takes("a/**/b", "a/b"));
    assert!(github_takes("a/**/b", "a/x/y/b"));
    assert!(!github_takes("a/**/b", "a/xb"));
    assert!(github_takes("a/**", "a/x"));
    assert!(github_takes("a/**", "a/x/y"));
    assert!(!github_takes("a/**", "a"));
    assert!(!github_takes("a/**", "b/a/x"));
    assert!(github_takes("**", "a/b"));
    assert!(github_takes("/**/x", "x"));
    assert!(github_takes("/**/x", "a/x"));
    assert!(github_takes("a/**/", "a/x"));
    assert_eq!(github("a/**/").unwrap(), "(?s)^a/.*$");
}

#[test]
fn github_reads_one_character_escapes_and_case_as_written() {
    assert!(github_takes("docs/?.md", "docs/a.md"));
    assert!(github_takes("docs/?.md", "docs/é.md"));
    assert!(!github_takes("docs/?.md", "docs/ab.md"));
    assert!(!github_takes("docs/?.md", "docs//.md"));
    assert!(github_takes("a\\*b", "a*b"));
    assert!(!github_takes("a\\*b", "axb"));
    assert!(github_takes("a\\", "a\\"));
    assert!(!github_takes("a.b", "axb"));
    assert!(!github_takes("Docs/", "docs/x"));
    assert!(github_takes("a\\ b/", "a b/x"));
}

#[test]
fn github_refuses_what_github_does_not_read() {
    assert_eq!(
        github("!x").unwrap_err(),
        "GitHub does not read a pattern starting with \"!\""
    );
    assert_eq!(
        github("\\#x").unwrap_err(),
        "GitHub does not read a pattern starting with \"\\#\""
    );
    assert_eq!(
        github("a[bc]").unwrap_err(),
        "GitHub does not read \"[ ]\" in a pattern"
    );
    for pattern in ["/", "a//b", "//"] {
        assert_eq!(
            github(pattern).unwrap_err(),
            "the pattern names no path, or holds \"//\"",
            "{pattern}"
        );
    }
    assert!(github("a#b").is_ok());
}

#[test]
fn gitlab_reads_its_documented_examples() {
    for path in ["a", "a/b/.hidden"] {
        assert!(gitlab_takes("*", path), "{path}");
    }
    assert!(gitlab_takes("README.md", "README.md"));
    assert!(gitlab_takes("README.md", "a/README.md"));
    assert!(gitlab_takes("/docs/README.md", "docs/README.md"));
    assert!(!gitlab_takes("/docs/README.md", "x/docs/README.md"));
    assert!(gitlab_takes("/docs/*.md", "docs/a.md"));
    assert!(!gitlab_takes("/docs/*.md", "docs/a/b.md"));
    assert!(gitlab_takes("/docs/**/index.md", "docs/index.md"));
    assert!(gitlab_takes("/docs/**/index.md", "docs/a/b/index.md"));
    assert!(gitlab_takes("/docs/", "docs/a"));
    assert!(gitlab_takes("/docs/", "docs/a/b"));
    assert!(!gitlab_takes("/docs/", "docs"));
    assert!(gitlab_takes("docs", "docs"));
    assert!(gitlab_takes("docs", "a/docs"));
    assert!(!gitlab_takes("docs", "docs/a"));
    assert!(gitlab_takes("/docs/**", "docs/a"));
    assert!(!gitlab_takes("/docs/**", "docs/a/b"));
    assert!(gitlab_takes("**/*.rb", "config/database.rb"));
    assert!(gitlab_takes(
        "**/*.rb",
        "app/controllers/users/stars_controller.rb"
    ));
    assert!(gitlab_takes("*.md", ".x.md"));
    assert!(gitlab_takes(
        "path\\ with\\ spaces/*.md",
        "path with spaces/a.md"
    ));
    assert!(!gitlab_takes("/Docs/", "docs/a"));
    assert_eq!(gitlab("/a/b"), "(?s)^a/b$");
}

#[test]
fn gitlab_reads_classes_as_fnmatch_does_never_across_a_slash() {
    assert!(gitlab_takes("/[abc].md", "a.md"));
    assert!(!gitlab_takes("/[abc].md", "d.md"));
    assert!(gitlab_takes("/[!abc].md", "d.md"));
    assert!(!gitlab_takes("/[!abc].md", "a.md"));
    assert!(gitlab_takes("/[^abc].md", "d.md"));
    assert!(!gitlab_takes("/x[!abc]y", "x/y"));
    assert!(!gitlab_takes("/x[/]y", "x/y"));
    assert!(!gitlab_takes("/x[.-0]y", "x/y"));
    assert!(gitlab_takes("/x[.-0]y", "x.y"));
    assert!(gitlab_takes("/[a-c]x", "bx"));
    assert!(!gitlab_takes("/[a-c]x", "dx"));
    assert!(gitlab_takes("/[]]x", "]x"));
    assert!(gitlab_takes("/[\\]]x", "]x"));
    assert!(gitlab_takes("/[a-]", "-"));
    assert!(gitlab_takes("/[a-]", "a"));
    assert!(!gitlab_takes("/[a-]", "b"));
    assert!(gitlab_takes("/[ab", "[ab"));
    assert!(gitlab_takes("/[\\", "[\\"));
    assert!(gitlab_takes("/[a-\\", "[a-\\"));
    assert!(gitlab_takes("/[é]", "é"));
    assert!(gitlab_takes("/[!]x]y", "zy"));
    assert!(!gitlab_takes("/[!]x]y", "]y"));
}

#[test]
fn forgejo_is_the_expression_between_anchors_ungrouped_as_forgejo_writes_it() {
    assert!(takes(&forgejo("src/.*"), "src/a/b"));
    assert!(!takes(&forgejo("src/.*"), "x/src/a"));
    assert!(takes(&forgejo("a|b"), "axx"));
    assert!(takes(&forgejo("a|b"), "xxb"));
    assert!(!takes(&forgejo("a|b"), "xbx"));
}
