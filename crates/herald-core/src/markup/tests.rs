use super::*;

const SAMPLE: &str = "# Added\n\nSome **bold**, *em*, ~~gone~~ and `code`.\n\n- one\n- two\n  - nested\n\n1. first\n2. second\n\n> quoted\n> line\n\n```rust\nlet x = 1 < 2;\n```\n\n[the release](https://example.org/r) and <https://example.org/auto>\n\n<b>raw</b> [bad](javascript:alert(1)) a_b*c\n\n---\n\nend";

#[test]
fn markdown_becomes_discord_markdown_with_its_text_escaped() {
    assert_eq!(
        convert(SAMPLE, Dialect::Discord),
        Text {
            body: "# Added\n\nSome **bold**, *em*, ~~gone~~ and `code`.\n\n- one\n- two\n  - nested\n\n1. first\n2. second\n\n> quoted\n> line\n\n```rust\nlet x = 1 < 2;\n```\n\n[the release](https://example.org/r) and https://example.org/auto\n\n\\<b\\>raw\\</b\\> bad a\\_b\\*c\n\n───\n\nend".to_owned(),
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
            "<h1>Added</h1>\n<p>Some <strong>bold</strong>, <em>em</em>, <del>gone</del> and <code>code</code>.</p>\n<ul>\n<li>one</li>\n<li>two\n<ul>\n<li>nested</li>\n</ul>\n</li>\n</ul>\n<ol>\n<li>first</li>\n<li>second</li>\n</ol>\n<blockquote>\n<p>quoted\nline</p>\n</blockquote>\n<pre><code class=\"language-rust\">let x = 1 &lt; 2;\n</code></pre>\n<p><a href=\"https://example.org/r\">the release</a> and <a href=\"https://example.org/auto\">https://example.org/auto</a></p>\n<p>&lt;b&gt;raw&lt;/b&gt; bad a_b*c</p>\n<hr />\n<p>end</p>"
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
        "[logo](https://example.org/logo.png)"
    );
    assert_eq!(
        convert("![](https://example.org/logo.png)", Dialect::Plain).body,
        "https://example.org/logo.png"
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
