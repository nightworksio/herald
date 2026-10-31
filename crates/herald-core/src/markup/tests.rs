use super::*;

const SAMPLE: &str = "# Added\n\nSome **bold**, *em*, ~~gone~~ and `code`.\n\n- one\n- two\n  - nested\n\n1. first\n2. second\n\n> quoted\n> line\n\n```rust\nlet x = 1 < 2;\n```\n\n[the release](https://example.org/r) and <https://example.org/auto>\n\n<b>raw</b> [bad](javascript:alert(1)) a_b*c\n\n---\n\nend";

#[test]
fn markdown_becomes_discord_markdown_with_its_text_escaped() {
    assert_eq!(
        convert(SAMPLE, Dialect::Discord),
        Text {
            body: "# Added\n\nSome **bold**\\, *em*\\, ~~gone~~ and `code`\\.\n\n- one\n- two\n  - nested\n\n1. first\n2. second\n\n> quoted\n> line\n\n```rust\nlet x = 1 < 2;\n```\n\nthe release (<https://example.org/r>) and <https://example.org/auto>\n\n\\<b\\>raw\\<\\/b\\> bad a\\_b\\*c\n\n───\n\nend".to_owned(),
            html: None,
        }
    );
}

#[test]
fn markdown_becomes_slack_mrkdwn_with_its_control_characters_escaped() {
    assert_eq!(
        convert(SAMPLE, Dialect::Slack),
        Text {
            body: "*Added*\n\nSome *bold*, _em_, ~gone~ and `code`.\n\n• one\n• two\n  • nested\n\n1. first\n2. second\n\n> quoted\n> line\n\n```\nlet x = 1 &lt; 2;\n```\n\n<https://example.org/r|the release> and <https://example.org/auto>\n\n&lt;b&gt;raw&lt;/b&gt; bad a_b*c\n\n───\n\nend".to_owned(),
            html: None,
        }
    );
}

#[test]
fn markdown_becomes_plain_text_with_links_spelled_out() {
    assert_eq!(
        convert(SAMPLE, Dialect::Plain),
        Text {
            body: "Added\n\nSome bold, em, gone and code.\n\n- one\n- two\n  - nested\n\n1. first\n2. second\n\n> quoted\n> line\n\nlet x = 1 < 2;\n\nthe release (https://example.org/r) and https://example.org/auto\n\n<b>raw</b> bad a_b*c\n\n───\n\nend".to_owned(),
            html: None,
        }
    );
}

#[test]
fn markdown_becomes_matrix_html_beside_a_plain_body() {
    let text = convert(SAMPLE, Dialect::Matrix);
    assert_eq!(text.body, convert(SAMPLE, Dialect::Plain).body);
    assert_eq!(
        text.html.as_deref(),
        Some(
            "<h1>Added</h1>\n<p>Some <strong>bold</strong>, <em>em</em>, <del>gone</del> and <code>code</code>.</p>\n<ul>\n<li>one</li>\n<li>two\n<ul>\n<li>nested</li>\n</ul>\n</li>\n</ul>\n<ol>\n<li>first</li>\n<li>second</li>\n</ol>\n<blockquote>\n<p>quoted\nline</p>\n</blockquote>\n<pre><code class=\"language-rust\">let x = 1 &lt; 2;\n</code></pre>\n<p>the release (<a href=\"https://example.org/r\">https://example.org/r</a>) and <a href=\"https://example.org/auto\">https://example.org/auto</a></p>\n<p>&lt;b&gt;raw&lt;/b&gt; bad a_b*c</p>\n<hr />\n<p>end</p>"
        )
    );
}

#[test]
fn discord_headings_stop_at_its_third_level() {
    assert_eq!(convert("##### deep", Dialect::Discord).body, "### deep");
    assert_eq!(convert("## two", Dialect::Discord).body, "## two");
}

#[test]
fn discord_inline_code_holding_a_backtick_takes_a_double_fence() {
    assert_eq!(convert("``a`b``", Dialect::Discord).body, "``a`b``");
    assert_eq!(convert("`` `a ``", Dialect::Discord).body, "`` `a ``");
}

#[test]
fn the_oracle_refuses_what_could_mention_or_link() {
    use super::inert::{inert, literal_inert};
    let text = |body: &str| Text {
        body: body.to_owned(),
        html: None,
    };
    let matrix = |html: &str| Text {
        body: String::new(),
        html: Some(html.to_owned()),
    };
    assert!(inert(&text("@everyone"), Dialect::Plain).is_err());
    assert!(inert(&text("[x"), Dialect::Discord).is_err());
    assert!(inert(&text("`x"), Dialect::Discord).is_err());
    assert!(inert(&text("<@&1>"), Dialect::Discord).is_err());
    assert!(inert(&matrix("<p"), Dialect::Matrix).is_err());
    assert!(inert(&text("x\n-# small"), Dialect::Discord).is_err());
    assert!(inert(&text(">>> quote"), Dialect::Discord).is_err());
    assert!(inert(&text("a & b"), Dialect::Slack).is_err());
    assert!(inert(&text("#tag"), Dialect::Plain).is_err());
    assert!(inert(&text("\u{FF20}everyone"), Dialect::Discord).is_err());
    let masked = Text {
        body: "https://a.example".to_owned(),
        html: Some("<a href=\"https://a.example\">b</a>".to_owned()),
    };
    assert!(inert(&masked, Dialect::Matrix).is_err());
    let unshown = Text {
        body: "nothing".to_owned(),
        html: Some("<a href=\"https://a.example\">https://a.example</a>".to_owned()),
    };
    assert!(inert(&unshown, Dialect::Matrix).is_err());
    assert!(inert(&text("a > b"), Dialect::Slack).is_err());
    assert!(inert(&text("<!channel>"), Dialect::Slack).is_err());
    assert!(inert(&matrix("<script>x</script>"), Dialect::Matrix).is_err());
    assert!(
        inert(
            &matrix("<a href=\"https://matrix.to/#/@a:b\">a</a>"),
            Dialect::Matrix
        )
        .is_err()
    );
    assert_eq!(inert(&matrix("<p>x</p>"), Dialect::Matrix), Ok(()));
    assert_eq!(
        inert(&convert("3. three", Dialect::Matrix), Dialect::Matrix),
        Ok(())
    );
    assert!(inert(&matrix("<ol start=\"x\">"), Dialect::Matrix).is_err());
    assert!(literal_inert(&text("a*b"), Dialect::Discord).is_err());
    assert!(literal_inert(&text("<https://x.example>"), Dialect::Slack).is_err());
    assert!(literal_inert(&matrix("<p>x</p>"), Dialect::Matrix).is_err());
}

#[test]
fn a_slack_link_cannot_end_early_or_split_on_its_url() {
    assert_eq!(
        convert("[x](https://example.org/a|b>c)", Dialect::Slack).body,
        "<https://example.org/a%7Cb%3Ec|x>"
    );
}

#[test]
fn a_mail_link_is_kept_and_an_image_becomes_its_link() {
    assert_eq!(
        convert("[mail](mailto:a@example.org)", Dialect::Plain).body,
        "mail (mailto:a@example.org)"
    );
    assert_eq!(
        convert("![logo](https://example.org/logo.png)", Dialect::Discord).body,
        "logo (<https://example.org/logo.png>)"
    );
    assert_eq!(
        convert("![](https://example.org/logo.png)", Dialect::Plain).body,
        "https://example.org/logo.png"
    );
}

#[test]
fn matrix_html_shows_each_links_url_once_and_keeps_its_formatting() {
    assert_eq!(
        convert(
            "[![i](https://img.example/i.png) **docs**](https://example.org/d)",
            Dialect::Matrix
        )
        .html
        .as_deref(),
        Some(
            "<p>i <strong>docs</strong> (<a href=\"https://example.org/d\">https://example.org/d</a>)</p>"
        )
    );
    assert_eq!(
        convert("3. three", Dialect::Matrix).html.as_deref(),
        Some("<ol start=\"3\">\n<li>three</li>\n</ol>")
    );
}

#[test]
fn a_bare_url_in_plain_text_is_kept_apart_from_what_follows() {
    assert_eq!(
        convert("<https://a.example>@x and more", Dialect::Plain).body,
        "https://a.example @\u{200B}x and more"
    );
    assert_eq!(
        convert("<https://a.example> then", Dialect::Plain).body,
        "https://a.example then"
    );
}

#[test]
fn a_link_inside_a_link_is_its_text_alone() {
    assert_eq!(
        convert(
            "[![logo](https://example.org/i.png) docs](https://example.org/d)",
            Dialect::Slack
        )
        .body,
        "<https://example.org/d|logo docs>"
    );
}

#[test]
fn a_quote_inside_a_list_is_quoted_inside_its_item() {
    assert_eq!(
        convert("- item\n\n  > said", Dialect::Plain).body,
        "- item\n> said"
    );
}

#[test]
fn a_loose_list_keeps_its_items_on_lines_of_their_own() {
    assert_eq!(
        convert("- one\n\n- two", Dialect::Plain).body,
        "- one\n- two"
    );
}

#[test]
fn an_ordered_list_counts_from_where_it_starts() {
    assert_eq!(
        convert("3. three\n4. four", Dialect::Slack).body,
        "3. three\n4. four"
    );
}

#[test]
fn a_hard_break_is_a_new_line() {
    assert_eq!(convert("one  \ntwo", Dialect::Discord).body, "one\ntwo");
}

#[test]
fn a_task_list_shows_its_boxes() {
    assert_eq!(
        convert("- [x] done\n- [ ] open", Dialect::Plain).body,
        "- ☑ done\n- ☐ open"
    );
}

#[test]
fn an_html_block_is_shown_as_its_text() {
    assert_eq!(
        convert("<div>\nhi\n</div>", Dialect::Slack).body,
        "&lt;div&gt;\nhi\n&lt;/div&gt;"
    );
}

#[test]
fn a_quote_keeps_its_heading_list_and_code_inside_it() {
    assert_eq!(
        convert(
            "> # Title\n>\n> - one\n>\n> ```\n> code\n> ```",
            Dialect::Discord
        )
        .body,
        "> # Title\n>\n> - one\n>\n> ```\n> code\n> ```"
    );
}

mod hostile {
    use proptest::prelude::*;

    use super::super::inert::{inert, literal_inert};
    use super::*;
    use crate::event::Draft;
    use crate::event::tests::draft;
    use crate::render::{Context, Form, Templates};
    use crate::{Event, RouteId};

    const DIALECTS: [Dialect; 4] = [
        Dialect::Discord,
        Dialect::Slack,
        Dialect::Matrix,
        Dialect::Plain,
    ];

    const PIECES: &[&str] = &[
        "@everyone",
        "@here",
        "@room",
        "@user@mastodon.social",
        "<@&123>",
        "<@123>",
        "<#1>",
        "<!channel>",
        "<!here|x>",
        "[x](https://evil.example)",
        "[https://good.example](https://evil.example)",
        "![i](javascript:alert(1))",
        "![i](https://img.example/i.png)",
        "<https://auto.example>",
        "<a href=\"https://x\">y</a>",
        "<script>x</script>",
        "https://matrix.to/#/@a:b",
        "[pill](https://matrix.to/#/@a:b)",
        "[x](<https://a b>)",
        "[x](https://a>b|c)",
        "[x](mailto:a@b.c)",
        "`",
        "``",
        "```",
        "~~~",
        "\n",
        "\n\n",
        "> ",
        "- ",
        "1. ",
        "# ",
        "**",
        "_",
        "|",
        "&",
        "&lt;",
        "\\",
        "[",
        "]",
        "(",
        ")",
        "- [x] ",
        "    ",
        "\t",
        "<@!1>",
        "<t:1:R>",
        "</cmd:1>",
        "<:e:1>",
        "<a:e:1>",
        "-# ",
        ">>> ",
        "<!subteam^S1>",
        "<@U1>",
        "<#C1>",
        "&amp;lt;",
        "&#64;everyone",
        "&#x40;here",
        "\u{FF20}everyone",
        "\u{FE6B}here",
        "#tag",
        "&lt;@1&gt;",
        "\u{200B}",
        "||spoiler||",
        "@room:example.org",
        "<https://a.example>@x",
        "[a](https://a.example?q=1&r=2)",
        "\t>>>> ",
    ];

    fn hostile() -> impl Strategy<Value = String> {
        let piece = prop_oneof![
            proptest::sample::select(PIECES).prop_map(str::to_owned),
            "[ -~]{0,6}",
            "\\PC{0,3}",
        ];
        proptest::collection::vec(piece, 0..24).prop_map(|pieces| pieces.concat())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn converted_markdown_mentions_no_one_and_links_nowhere_unshown(markdown in hostile()) {
            for dialect in DIALECTS {
                let text = convert(&markdown, dialect);
                prop_assert_eq!(inert(&text, dialect), Ok(()), "{:?} in {:?}", markdown, dialect);
            }
        }

        #[test]
        fn literal_text_is_only_its_letters(text in hostile()) {
            for dialect in DIALECTS {
                let shown = literal(&text, dialect);
                prop_assert_eq!(literal_inert(&shown, dialect), Ok(()), "{:?} in {:?}", text, dialect);
            }
        }

        #[test]
        fn a_hostile_event_through_a_markdown_template_stays_inert(title in hostile(), body in hostile()) {
            let Ok(event) = Event::normalise(Draft { title: format!("t{title}"), body, ..draft() }) else {
                return Ok(());
            };
            let templates = Templates::compile(&[(
                "body",
                "## {{ event.title }}\n\n[release]({{ event.links.release }}) {{ event.project }}\n\n{{ event.body }}",
                Form::Markdown,
            )])
            .unwrap();
            let route = RouteId::new("r");
            let markdown = templates.render("body", &Context { event: &event, route: &route, herald: "0" }).unwrap();
            for dialect in DIALECTS {
                prop_assert_eq!(inert(&convert(&markdown, dialect), dialect), Ok(()), "{:?} in {:?}", markdown, dialect);
            }
        }
    }
}

#[test]
fn an_at_sign_cannot_start_a_mention() {
    assert_eq!(
        convert("@everyone `@here`", Dialect::Discord).body,
        "@\u{200B}everyone `@\u{200B}here`"
    );
    assert_eq!(
        convert("@user@example.social", Dialect::Plain).body,
        "@\u{200B}user@\u{200B}example.social"
    );
    let matrix = convert("@room `@room`", Dialect::Matrix);
    assert_eq!(matrix.body, "@\u{200B}room @\u{200B}room");
    assert_eq!(
        matrix.html.as_deref(),
        Some("<p>@\u{200B}room <code>@\u{200B}room</code></p>")
    );
}

#[test]
fn a_matrix_pill_link_is_its_text_alone_and_an_image_a_link() {
    let markdown = "[pill](https://matrix.to/#/@a:b) ![logo](https://example.org/i.png)";
    assert_eq!(
        convert(markdown, Dialect::Matrix).html.as_deref(),
        Some(
            "<p>pill logo (<a href=\"https://example.org/i.png\">https://example.org/i.png</a>)</p>"
        )
    );
    assert_eq!(
        convert("[pill](https://matrix.to/#/@a:b)", Dialect::Discord).body,
        "pill (<https://matrix.to/#/@a:b>)"
    );
}

#[test]
fn a_url_is_encoded_past_anything_that_could_close_it() {
    assert_eq!(
        convert("[x](<https://a.example/b c\"d>)", Dialect::Plain).body,
        "x (https://a.example/b%20c%22d)"
    );
}

#[test]
fn a_code_language_that_is_no_language_is_dropped() {
    assert_eq!(
        convert("```@everyone\ncode\n```", Dialect::Discord).body,
        "```\ncode\n```"
    );
    assert_eq!(
        convert("```rust extra\ncode\n```", Dialect::Discord).body,
        "```rust\ncode\n```"
    );
    assert_eq!(
        convert("```a\"b\ncode\n```", Dialect::Matrix)
            .html
            .as_deref(),
        Some("<pre><code>code\n</code></pre>")
    );
}

#[test]
fn code_cannot_close_its_own_fence() {
    assert_eq!(
        convert("~~~\na```b\n~~~", Dialect::Discord).body,
        "```\na`\u{200B}`\u{200B}`b\n```"
    );
    assert_eq!(
        convert("~~~\na```b\n~~~", Dialect::Slack).body,
        "```\na`\u{200B}`\u{200B}`b\n```"
    );
    assert_eq!(convert("``a`b``", Dialect::Slack).body, "a`b");
}

#[test]
fn literal_text_is_escaped_for_each_dialect() {
    let text = "a*b @c <d>";
    assert_eq!(
        literal(text, Dialect::Discord).body,
        "a\\*b @\u{200B}c \\<d\\>"
    );
    assert_eq!(literal(text, Dialect::Slack).body, "a*b @c &lt;d&gt;");
    assert_eq!(literal(text, Dialect::Plain).body, "a*b @\u{200B}c <d>");
    let matrix = literal(text, Dialect::Matrix);
    assert_eq!(matrix.body, "a*b @\u{200B}c <d>");
    assert_eq!(matrix.html.as_deref(), Some("a*b @\u{200B}c &lt;d&gt;"));
}
