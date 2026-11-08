use std::collections::BTreeMap;

use super::*;

fn worded(pairs: &[(Phrase, &str)]) -> BTreeMap<Phrase, String> {
    pairs
        .iter()
        .map(|(phrase, text)| (*phrase, (*text).to_owned()))
        .collect()
}

#[test]
fn each_phrase_is_named_as_wording_writes_it() {
    for phrase in Phrase::ALL {
        let read: Phrase = serde_json::from_value(serde_json::json!(phrase.name())).unwrap();
        assert_eq!(read, phrase);
        assert_ne!(phrase.default_text(), "");
    }
}

#[test]
fn herald_says_what_it_always_said_where_nothing_is_worded() {
    let wording = Wording::default();
    assert_eq!(
        wording.say(&Said::Subscribed("a*b".to_owned())),
        "You will be mentioned when a\\*b announces a release."
    );
    assert_eq!(
        wording.say(&Said::Unchosen(Unchosen::Unknown {
            named: "x".to_owned(),
            known: vec!["a".to_owned(), "b".to_owned()],
        })),
        "x is not announced here; name one of a, b."
    );
    assert_eq!(
        wording.say(&Said::Disabled(Command::Latest)),
        "The command latest is not answered here."
    );
    assert_eq!(
        Said::Unavailable.to_string(),
        wording.say(&Said::Unavailable)
    );
    assert_eq!(wording.date(Timestamp::UNIX_EPOCH), "1970-01-01");
    assert_eq!(
        wording.describe(Command::Changelog),
        "Show the notes of what a project announced last"
    );
}

#[test]
fn a_worded_phrase_is_said_with_its_values_escaped() {
    let wording = Wording::of(Some(&worded(&[
        (
            Phrase::Subscribed,
            "Je krijgt een melding bij {{ project }}.",
        ),
        (Phrase::Many, "Kies uit {{ projects }}."),
        (Phrase::Date, "%d-%m-%Y"),
        (Phrase::DescribeLatest, "Toon de laatste"),
    ])));
    assert_eq!(
        wording.say(&Said::Subscribed("a_b".to_owned())),
        "Je krijgt een melding bij a\\_b."
    );
    assert_eq!(
        wording.say(&Said::Unchosen(Unchosen::Many(vec![
            "a".to_owned(),
            "b".to_owned()
        ]))),
        "Kies uit a, b."
    );
    assert_eq!(wording.date(Timestamp::UNIX_EPOCH), "01-01-1970");
    assert_eq!(wording.describe(Command::Latest), "Toon de laatste");
    assert_eq!(
        wording.say(&Said::Unsubscribed("p".to_owned())),
        "You will no longer be mentioned when p announces a release."
    );
}

#[test]
fn a_phrase_that_cannot_be_said_is_refused_and_one_that_fails_later_falls_back() {
    for (phrase, text, refused) in [
        (
            Phrase::Subscribed,
            "{{ project",
            "wording.subscribed is not a template",
        ),
        (
            Phrase::Date,
            "%Y-%",
            "wording.date is not a strftime pattern",
        ),
        (
            Phrase::DescribeShare,
            "",
            "wording.describe_share takes 1 to 100 characters",
        ),
    ] {
        let error = Wording::check(&worded(&[(phrase, text)])).unwrap_err();
        assert!(error.starts_with(refused), "{error}");
    }
    let long = "a".repeat(101);
    assert!(Wording::check(&worded(&[(Phrase::DescribeGroup, long.as_str())])).is_err());
    assert_eq!(
        Wording::check(&worded(&[(Phrase::Nothing, "Niets.")])),
        Ok(())
    );
    let failing = Wording::of(Some(&worded(&[(Phrase::Unavailable, "{{ 1 / 0 }}")])));
    assert_eq!(
        failing.say(&Said::Unavailable),
        Phrase::Unavailable.default_text()
    );
}
