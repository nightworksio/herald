use proptest::prelude::*;

use super::*;

fn text(value: &str) -> Bare {
    Bare::String(value.to_owned())
}

fn item(bare: Bare) -> Item {
    Item {
        bare,
        parameters: Vec::new(),
    }
}

#[test]
fn a_signature_input_reads_as_an_inner_list_with_its_parameters() {
    let read = dictionary(
        r#"sig1=("@method" "@target-uri" "content-digest");created=1618884473;keyid="k";alg="ed25519""#,
    );
    assert_eq!(
        read,
        Some(vec![(
            "sig1".to_owned(),
            Member::Inner(
                vec![
                    item(text("@method")),
                    item(text("@target-uri")),
                    item(text("content-digest")),
                ],
                vec![
                    ("created".to_owned(), Bare::Integer(1_618_884_473)),
                    ("keyid".to_owned(), text("k")),
                    ("alg".to_owned(), text("ed25519")),
                ],
            ),
        )])
    );
}

#[test]
fn every_bare_item_reads() {
    let read = dictionary(
        r#"a=-12, b="q\"s\\", c=tok/en:1, s=*star, d=:aGk=:, e=?0, f=?1, g;p, h=( ), i=(1 2;x);y=?0"#,
    );
    assert_eq!(
        read,
        Some(vec![
            ("a".to_owned(), Member::Item(item(Bare::Integer(-12)))),
            ("b".to_owned(), Member::Item(item(text("q\"s\\")))),
            (
                "c".to_owned(),
                Member::Item(item(Bare::Token("tok/en:1".to_owned())))
            ),
            (
                "s".to_owned(),
                Member::Item(item(Bare::Token("*star".to_owned())))
            ),
            (
                "d".to_owned(),
                Member::Item(item(Bare::Bytes(b"hi".to_vec())))
            ),
            ("e".to_owned(), Member::Item(item(Bare::Boolean(false)))),
            ("f".to_owned(), Member::Item(item(Bare::Boolean(true)))),
            (
                "g".to_owned(),
                Member::Item(Item {
                    bare: Bare::Boolean(true),
                    parameters: vec![("p".to_owned(), Bare::Boolean(true))],
                })
            ),
            ("h".to_owned(), Member::Inner(Vec::new(), Vec::new())),
            (
                "i".to_owned(),
                Member::Inner(
                    vec![
                        item(Bare::Integer(1)),
                        Item {
                            bare: Bare::Integer(2),
                            parameters: vec![("x".to_owned(), Bare::Boolean(true))],
                        },
                    ],
                    vec![("y".to_owned(), Bare::Boolean(false))],
                )
            ),
        ])
    );
}

#[test]
fn a_later_member_or_parameter_of_a_name_replaces_the_earlier() {
    assert_eq!(
        dictionary("a=1;p=1;q=2;p=3, b=2,a=4"),
        Some(vec![
            ("b".to_owned(), Member::Item(item(Bare::Integer(2)))),
            ("a".to_owned(), Member::Item(item(Bare::Integer(4)))),
        ])
    );
    assert_eq!(
        dictionary("a=1;p=1;q=2;p=3"),
        Some(vec![(
            "a".to_owned(),
            Member::Item(Item {
                bare: Bare::Integer(1),
                parameters: vec![
                    ("q".to_owned(), Bare::Integer(2)),
                    ("p".to_owned(), Bare::Integer(3)),
                ],
            })
        )])
    );
}

#[test]
fn surrounding_spaces_and_tabs_between_members_are_skipped() {
    assert_eq!(
        dictionary("  a=1 ,\tb=2  "),
        Some(vec![
            ("a".to_owned(), Member::Item(item(Bare::Integer(1)))),
            ("b".to_owned(), Member::Item(item(Bare::Integer(2)))),
        ])
    );
    assert_eq!(dictionary(""), Some(Vec::new()));
    assert_eq!(
        dictionary("a=-123456789012345"),
        Some(vec![(
            "a".to_owned(),
            Member::Item(item(Bare::Integer(-123_456_789_012_345)))
        )])
    );
}

#[test]
fn what_is_no_dictionary_reads_as_none() {
    for broken in [
        "A=1",
        "1a=1",
        "a=1,",
        "a=1, ",
        "a=1 b=2",
        "a=\"open",
        "a=\"bad\\n\"",
        "a=\"tab\t\"",
        "a=:aGk=",
        "a=:!!:",
        "a=?2",
        "a=?",
        "a=-",
        "a=1.5",
        "a=1234567890123456",
        "a=(1 2",
        "a=(1\"x\")",
        "a=@",
        "a=",
        "a=1;",
        "a=1;P=1",
        "a=1;p=@",
        "a=(1);P",
        "a=(@)",
        "a=,b=1",
    ] {
        assert_eq!(dictionary(broken), None, "{broken}");
    }
}

#[test]
fn bare_items_and_parameters_are_written_as_they_read() {
    assert_eq!(written(&Bare::Integer(-3)), "-3");
    assert_eq!(written(&text("a\"b\\c")), "\"a\\\"b\\\\c\"");
    assert_eq!(written(&Bare::Token("t:1".to_owned())), "t:1");
    assert_eq!(written(&Bare::Bytes(b"hi".to_vec())), ":aGk=:");
    assert_eq!(written(&Bare::Boolean(false)), "?0");
    assert_eq!(written(&Bare::Boolean(true)), "?1");
    assert_eq!(
        written_inner(
            &[text("@method"), text("@path")],
            &[
                ("created".to_owned(), Bare::Integer(5)),
                ("flag".to_owned(), Bare::Boolean(true)),
                ("off".to_owned(), Bare::Boolean(false)),
                ("keyid".to_owned(), text("k")),
            ]
        ),
        "(\"@method\" \"@path\");created=5;flag;off=?0;keyid=\"k\""
    );
    assert_eq!(written_inner(&[], &[]), "()");
}

proptest! {
    #[test]
    fn a_written_string_reads_back(value in "[ -~]{0,40}") {
        let field = format!("a={}", written(&text(&value)));
        prop_assert_eq!(
            dictionary(&field),
            Some(vec![("a".to_owned(), Member::Item(item(text(&value))))])
        );
    }
}
