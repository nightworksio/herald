use jiff::Timestamp;

use super::*;
use crate::{Changed, MOST_AREAS_READ, Owned};

const MOST: (usize, usize) = (128 * 1024, MOST_AREAS_READ);

fn key() -> CodeownersKey {
    CodeownersKey {
        source: SourceName::new("forge"),
        repository: "o/r".to_owned(),
        reference: Some("main".to_owned()),
    }
}

fn kept(dialect: Dialect, text: &str) -> (Codeowners, KeptCodeowners) {
    let read = Codeowners::read(dialect, text, MOST.1);
    let kept = KeptCodeowners::of(
        key(),
        ("CODEOWNERS", "\"e1\""),
        &read,
        (MOST, Timestamp::UNIX_EPOCH),
    )
    .expect("a small file is kept");
    (read, kept)
}

fn owning(read: &Codeowners, paths: &[&str]) -> Owned {
    read.owning(Some(&Changed {
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        whole: true,
    }))
}

#[test]
fn a_kept_file_owns_each_path_as_the_file_read() {
    let files = [
        (Dialect::GitHub, "* @a\ndocs/ @org/docs\nx[y] @b\n"),
        (
            Dialect::GitLab,
            "* @all\n[Docs][2] @docs\ndocs/\n!docs/x.md\n^[DB] @@maintainer\ndb/\n",
        ),
        (Dialect::Forgejo, ".*\\.md @org/docs\n!\\.go$ @bob\n"),
    ];
    for (dialect, text) in files {
        let (read, kept) = kept(dialect, text);
        let again = kept.file().expect("a kept file reads back");
        for paths in [
            &["docs/a.md"][..],
            &["db/s.sql", "x.go"],
            &["docs/x.md"],
            &[],
        ] {
            assert_eq!(
                owning(&again, paths),
                owning(&read, paths),
                "{dialect:?} {paths:?}"
            );
        }
        assert_eq!(again.skipped(), read.skipped());
        assert_eq!(again.refused(), read.refused());
        assert_eq!(again.undecided(), read.undecided());
        assert_eq!(again.form(), read.form());
        assert!(kept.fits(dialect, MOST));
    }
}

#[test]
fn a_refused_or_empty_file_is_kept_as_refused_or_empty() {
    let refused = Codeowners::within(Dialect::GitHub, b"* @a\n", (1, MOST.1));
    let kept = KeptCodeowners::of(
        key(),
        ("CODEOWNERS", "e"),
        &refused,
        (MOST, Timestamp::UNIX_EPOCH),
    )
    .expect("a refused file is kept");
    let again = kept.file().expect("a refused file reads back");
    assert_eq!(again.refused(), refused.refused());
    assert_eq!(owning(&again, &["a"]), Owned::default());
    let (empty, kept) = kept_empty();
    assert_eq!(
        kept.file().expect("an empty file reads back").form(),
        empty.form()
    );
}

fn kept_empty() -> (Codeowners, KeptCodeowners) {
    kept(Dialect::GitHub, "# nothing\n")
}

#[test]
fn a_record_longer_than_the_file_bound_is_not_kept() {
    let read = Codeowners::read(Dialect::GitHub, "* @a\n", MOST.1);
    let at = Timestamp::UNIX_EPOCH;
    let written = serde_json::to_vec(
        &KeptCodeowners::of(key(), ("CODEOWNERS", "e"), &read, ((999, MOST.1), at)).expect("kept"),
    )
    .expect("written")
    .len();
    assert!((100..=999).contains(&written), "{written}");
    let within = KeptCodeowners::of(key(), ("CODEOWNERS", "e"), &read, ((written, MOST.1), at));
    assert!(within.is_some());
    let past = KeptCodeowners::of(
        key(),
        ("CODEOWNERS", "e"),
        &read,
        ((written - 1, MOST.1), at),
    );
    assert_eq!(past, None);
}

#[test]
fn a_kept_file_fits_only_the_dialect_bounds_and_form_it_was_read_under() {
    let (_, kept) = kept(Dialect::GitLab, "* @a\n");
    assert!(kept.fits(Dialect::GitLab, MOST));
    assert!(!kept.fits(Dialect::GitHub, MOST));
    assert!(!kept.fits(Dialect::GitLab, (MOST.0 + 1, MOST.1)));
    assert!(!kept.fits(Dialect::GitLab, (MOST.0, MOST.1 - 1)));
    let older = KeptCodeowners {
        form: KEPT_CODEOWNERS - 1,
        ..kept
    };
    assert!(!older.fits(Dialect::GitLab, MOST));
}

#[test]
fn a_kept_file_whose_patterns_do_not_pair_or_compile_reads_back_as_none() {
    let (_, kept) = kept(Dialect::GitHub, "a @x\nb @y\n");
    let mut unpaired = kept.clone();
    unpaired.file.patterns.pop();
    assert!(unpaired.file().is_none());
    let mut broken = kept;
    broken.file.patterns[0] = "(".to_owned();
    assert!(broken.file().is_none());
}

#[test]
fn a_kept_file_holding_more_lines_or_areas_than_it_was_read_under_reads_back_as_none() {
    let (_, kept) = kept(
        Dialect::GitHub,
        "a @x @y
",
    );
    assert!(kept.file().is_some());
    let mut named = kept.clone();
    named.most.1 = 2;
    assert!(named.file().is_some());
    named.most.1 = 1;
    assert!(named.file().is_none());
    let mut long = kept.clone();
    let (rule, pattern) = (long.file.rules[0].clone(), long.file.patterns[0].clone());
    long.most.1 = usize::MAX;
    long.file.rules = vec![rule; MOST_CODEOWNERS_LINES];
    long.file.patterns = vec![pattern; MOST_CODEOWNERS_LINES];
    assert!(long.file().is_some());
    long.file.rules.push(long.file.rules[0].clone());
    long.file.patterns.push(long.file.patterns[0].clone());
    assert!(long.file().is_none());
}

#[test]
fn the_kept_form_is_pinned_to_its_number() {
    let (_, kept) = kept(Dialect::GitLab, "[Docs] @docs\ndocs/ @@maintainer\n");
    let written = serde_json::to_value(&kept).expect("written");
    assert_eq!(
        written,
        serde_json::json!({
            "key": {"source": "forge", "repository": "o/r", "reference": "main"},
            "form": 1,
            "path": "CODEOWNERS",
            "etag": "\"e1\"",
            "at": "1970-01-01T00:00:00Z",
            "most": [131_072, 200],
            "file": {
                "dialect": "gitlab",
                "rules": [{
                    "owners": [{"kind": "role", "written": "@@maintainer"}],
                    "negative": false,
                    "section": 1
                }],
                "patterns": ["(?s)^(?:.*/)?docs/(?:.*/)?[^/]*$"],
                "sections": ["codeowners", "Docs"],
                "skipped": [],
                "refused": null
            }
        }),
        "a change to how a file is read changes what is kept: raise KEPT_CODEOWNERS with it"
    );
    assert_eq!(KEPT_CODEOWNERS, 1);
}
