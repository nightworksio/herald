use proptest::prelude::*;

use super::*;

fn room(first: usize, rest: usize) -> Room {
    Room {
        measure: Measure::Chars,
        first,
        rest,
    }
}

fn bodies(parts: &[Text]) -> Vec<&str> {
    parts.iter().map(|part| part.body.as_str()).collect()
}

#[test]
fn a_text_that_fits_is_one_part() {
    assert_eq!(
        bodies(&chunk("one\n\ntwo", Dialect::Plain, room(100, 100))),
        ["one\n\ntwo"]
    );
}

#[test]
fn an_empty_text_is_one_empty_part() {
    assert_eq!(bodies(&chunk("", Dialect::Plain, room(100, 100))), [""]);
    assert_eq!(
        bodies(&chunk("  \n\n ", Dialect::Plain, room(100, 100))),
        [""]
    );
}

#[test]
fn blocks_are_packed_into_parts_with_the_first_part_roomier() {
    let markdown = "aaaa\n\nbbbb\n\ncccc\n\ndddd";
    assert_eq!(
        bodies(&chunk(markdown, Dialect::Plain, room(10, 4))),
        ["aaaa\n\nbbbb", "cccc", "dddd"]
    );
}

#[test]
fn a_long_paragraph_is_cut_at_lines_then_words() {
    let markdown = "one two three\nfour five six";
    assert_eq!(
        bodies(&chunk(markdown, Dialect::Plain, room(13, 13))),
        ["one two three", "four five six"]
    );
    assert_eq!(
        bodies(&chunk("one two three", Dialect::Plain, room(7, 7))),
        ["one two", "three"]
    );
}

#[test]
fn a_long_word_is_cut_between_graphemes_and_never_inside_one() {
    let family = "👨\u{200d}👩\u{200d}👧";
    let markdown = format!("{family}{family}{family}");
    let parts = chunk(&markdown, Dialect::Plain, room(5, 5));
    assert_eq!(bodies(&parts), [family, family, family]);
}

#[test]
fn a_grapheme_longer_than_a_part_is_cut_between_its_characters() {
    let cluster = format!("e{}", "\u{301}".repeat(9));
    let parts = chunk(&cluster, Dialect::Plain, room(4, 4));
    assert_eq!(
        parts
            .iter()
            .map(|part| part.body.chars().count())
            .collect::<Vec<_>>(),
        [4, 4, 2]
    );
}

#[test]
fn a_code_block_whose_opening_does_not_fit_loses_its_language() {
    let markdown = format!("```{}\nlet a = 1;\n```", "x".repeat(40));
    assert_eq!(
        bodies(&chunk(&markdown, Dialect::Discord, room(20, 20))),
        ["```\nlet a = 1;\n```"]
    );
}

#[test]
fn a_code_block_cut_in_two_is_fenced_on_both_sides() {
    let markdown = "```rust\nlet a = 1;\nlet b = 2;\n```";
    let parts = chunk(markdown, Dialect::Discord, room(24, 24));
    assert_eq!(
        bodies(&parts),
        ["```rust\nlet a = 1;\n```", "```rust\nlet b = 2;\n```"]
    );
}

#[test]
fn an_unclosed_code_block_is_closed_where_it_is_cut() {
    let markdown = "~~~\nlet a = 1;\nlet b = 2;";
    let parts = chunk(markdown, Dialect::Discord, room(18, 18));
    assert_eq!(
        bodies(&parts),
        ["```\nlet a = 1;\n```", "```\nlet b = 2;\n```"]
    );
}

#[test]
fn a_block_with_markup_inside_stays_whole() {
    let markdown = "some **bold** text\n\n- one\n- *two*";
    assert_eq!(
        bodies(&chunk(markdown, Dialect::Discord, room(20, 20))),
        ["some **bold** text", "- one\n- *two*"]
    );
}

#[test]
fn a_rule_between_blocks_is_a_block_of_its_own() {
    assert_eq!(
        bodies(&chunk("aaaa\n\n---\n\nbbbb", Dialect::Plain, room(4, 4))),
        ["aaaa", "───", "bbbb"]
    );
}

#[test]
fn length_is_counted_as_the_destination_counts_it_html_included() {
    let text = Text {
        body: "é".to_owned(),
        html: Some("<p>é</p>".to_owned()),
    };
    assert_eq!(size(&text, Measure::Chars), 1 + 8);
    assert_eq!(size(&text, Measure::Bytes), 2 + 9);
    assert_eq!(
        size(
            &Text {
                body: "é".to_owned(),
                html: None
            },
            Measure::Bytes
        ),
        2
    );
}

#[test]
fn matrix_parts_are_measured_with_their_html() {
    let parts = chunk(
        "one\n\ntwo",
        Dialect::Matrix,
        Room {
            measure: Measure::Bytes,
            first: 20,
            rest: 20,
        },
    );
    assert_eq!(bodies(&parts), ["one", "two"]);
    assert_eq!(parts[0].html.as_deref(), Some("<p>one</p>"));
}

fn word() -> impl Strategy<Value = String> {
    "[a-z]{1,12}"
}

fn paragraphs() -> impl Strategy<Value = Vec<Vec<String>>> {
    proptest::collection::vec(proptest::collection::vec(word(), 1..30), 1..8)
}

proptest! {
    #[test]
    fn no_part_passes_its_limit_and_no_word_is_lost(
        paragraphs in paragraphs(),
        first in 12usize..200,
        rest in 12usize..200,
    ) {
        let markdown = paragraphs.iter().map(|words| words.join(" ")).collect::<Vec<_>>().join("\n\n");
        let parts = chunk(&markdown, Dialect::Plain, room(first, rest));
        for (index, part) in parts.iter().enumerate() {
            let limit = if index == 0 { first } else { rest };
            prop_assert!(size(part, Measure::Chars) <= limit, "part {} is {:?}", index, part.body);
        }
        let words: Vec<&str> = parts.iter().flat_map(|part| part.body.split_whitespace()).collect();
        let expected: Vec<&str> = paragraphs.iter().flatten().map(String::as_str).collect();
        prop_assert_eq!(words, expected);
    }

    #[test]
    fn every_part_of_a_cut_code_block_is_fenced(
        lines in proptest::collection::vec("[a-z ]{1,30}", 1..40),
        limit in 40usize..300,
    ) {
        let markdown = format!("```\n{}\n```", lines.join("\n"));
        for part in chunk(&markdown, Dialect::Discord, room(limit, limit)) {
            prop_assert!(part.body.starts_with("```\n") && part.body.ends_with("\n```"), "{:?}", part.body);
            prop_assert!(size(&part, Measure::Chars) <= limit, "{:?}", part.body);
        }
    }
}
