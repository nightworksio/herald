use herald_core::{Codeowners, Dialect, MOST_AREAS_READ};

use super::*;

fn found(text: &str, most: usize) -> Found {
    found_within(text, (most, MOST_AREAS_READ))
}

fn found_within(text: &str, most: (usize, usize)) -> Found {
    Found {
        path: ".github/CODEOWNERS".to_owned(),
        file: Codeowners::within(Dialect::GitHub, text.as_bytes(), most),
        etag: None,
    }
}

fn places() -> Vec<String> {
    vec!["CODEOWNERS".to_owned(), "docs/CODEOWNERS".to_owned()]
}

#[test]
fn doctor_names_each_skipped_line_by_number_and_why_and_counts_past_the_most() {
    assert_eq!(
        said("repo", Ok(Some(found("* @org/web\n", 1024))), &places()),
        Vec::<String>::new()
    );
    let skipped = said(
        "repo",
        Ok(Some(found("* @org/web\n[\n\n!x @a\n", 1024))),
        &places(),
    );
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    assert!(
        skipped[0].starts_with("The source repo skips line 2 of its CODEOWNERS file .github/CODEOWNERS, which matches no path: "),
        "{}",
        skipped[0]
    );
    assert!(skipped[1].contains("skips line 4 of"), "{}", skipped[1]);
    let named: String = (0..MOST_SKIPPED_NAMED).map(|_| "[\n").collect();
    assert_eq!(
        said("repo", Ok(Some(found(&named, 1 << 20))), &places()).len(),
        MOST_SKIPPED_NAMED
    );
    let more: String = (0..MOST_SKIPPED_NAMED + 3).map(|_| "[\n").collect();
    let counted = said("repo", Ok(Some(found(&more, 1 << 20))), &places());
    assert_eq!(counted.len(), MOST_SKIPPED_NAMED + 1);
    assert_eq!(
        counted.last().map(String::as_str),
        Some("The source repo skips 3 more lines of its CODEOWNERS file .github/CODEOWNERS.")
    );
}

#[test]
fn doctor_says_a_file_refused_missing_or_unread_without_its_forge_s_words() {
    let refused = said("repo", Ok(Some(found("* @org/web\n", 4))), &places());
    assert_eq!(refused.len(), 1);
    assert!(
        refused[0].starts_with("The source repo reads none of its CODEOWNERS file .github/CODEOWNERS, so its pull requests fall in no area: it is longer than 4 bytes"),
        "{}",
        refused[0]
    );
    assert_eq!(
        said("repo", Ok(None), &places()),
        [
            "The source repo finds no CODEOWNERS file at CODEOWNERS, docs/CODEOWNERS on the default branch, so its pull requests fall in no area."
        ]
    );
    let secret = "https://api.github.com/repos/acme/secret-plans 500";
    assert_eq!(
        said(
            "repo",
            Err(Unpolled::Unreachable(secret.to_owned())),
            &places()
        ),
        ["The source repo could not read its CODEOWNERS file: its forge could not be reached."]
    );
    assert_eq!(
        said(
            "repo",
            Err(Unpolled::Unreadable(secret.to_owned())),
            &places()
        ),
        [
            "The source repo could not read its CODEOWNERS file: its forge's answer could not be read."
        ]
    );
}

#[test]
fn doctor_names_most_areas_where_a_file_names_more_areas_than_the_source_reads() {
    let refused = said(
        "repo",
        Ok(Some(found_within("a @x\nb @y\n", (1024, 1)))),
        &places(),
    );
    assert_eq!(
        refused,
        [
            "The source repo reads none of its CODEOWNERS file .github/CODEOWNERS, so its pull requests fall in no area: it names 2 areas, and the source reads at most 1; raise most_areas, at most 2000."
        ]
    );
}

#[test]
fn doctor_says_a_file_the_store_keeps_no_record_of_is_read_in_full_every_round() {
    let codeowning = |most: usize| Codeowning {
        paths: places(),
        reference: herald_core::CodeownersRef::Default,
        most,
        most_areas: MOST_AREAS_READ,
    };
    let key = || CodeownersKey {
        source: SourceName::new("repo"),
        repository: "o/r".to_owned(),
        reference: None,
    };
    let at = Timestamp::UNIX_EPOCH;
    let unvalidated = found("* @org/web\n", 1024);
    assert_eq!(
        unkept("repo", (key(), &unvalidated), (&codeowning(1024), at)).as_deref(),
        Some(
            "The source repo's forge gives its CODEOWNERS file .github/CODEOWNERS no validator, so the store keeps no record of it and it is read in full every round."
        )
    );
    let validated = |text: &str, most: usize| {
        let mut validated = found(text, most);
        validated.etag = Some("\"1\"".to_owned());
        validated
    };
    assert_eq!(
        unkept(
            "repo",
            (key(), &validated("* @org/web\n", 1024)),
            (&codeowning(1024), at)
        ),
        None
    );
    assert_eq!(
        unkept(
            "repo",
            (key(), &validated("* @org/web\n", 64)),
            (&codeowning(64), at)
        )
        .as_deref(),
        Some(
            "The store keeps no record of the source repo's CODEOWNERS file .github/CODEOWNERS, so it is read in full every round: the record would be longer than its most_codeowners of 64 bytes."
        )
    );
    assert_eq!(
        unkept(
            "repo",
            (key(), &found("* @org/web\n", 4)),
            (&codeowning(4), at)
        ),
        None
    );
}
