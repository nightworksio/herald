use super::*;
use proptest::prelude::*;
use pulldown_cmark::{Event, Parser};

fn tags(markdown: &str) -> Vec<String> {
    Parser::new(markdown)
        .filter_map(|event| match event {
            Event::Html(tag) | Event::InlineHtml(tag) => Some(tag.into_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn paragraphs_headings_and_emphasis_become_markdown() {
    assert_eq!(
        markdown(
            "<h2>What&rsquo;s new</h2><p>A <strong>bold</strong> and <em>quiet</em>\n   change.</p><p>Second<br>line</p>"
        ),
        "## What’s new\n\nA **bold** and *quiet* change.\n\nSecond  \nline"
    );
    assert_eq!(
        markdown("<b>b</b><i>i</i><del>d</del><hr><s>s</s><strike>t</strike>"),
        "**b***i*~~d~~\n\n---\n\n~~s~~~~t~~"
    );
    assert_eq!(markdown("<h7>x</h7>"), "x");
}

#[test]
fn code_and_preformatted_text_keep_what_they_hold() {
    assert_eq!(
        markdown(
            "<p>Run <code>herald *init*</code>.</p><pre><code>let a = 1;\n  let b = *a;</code></pre>after"
        ),
        "Run `herald *init*`.\n\n```\nlet a = 1;\n  let b = *a;\n```\n\nafter"
    );
    assert_eq!(markdown("<pre>x\n</pre>"), "```\nx\n```");
    assert_eq!(markdown("</code></pre>text"), "text");
    assert_eq!(markdown("<code>a</code>*b*"), "`a`\\*b\\*");
    assert_eq!(markdown("<code>a<span>b</span>c</code>"), "`abc`");
    assert_eq!(markdown("<code></code>x"), "x");
}

#[test]
fn backticks_in_code_never_end_it_early() {
    let converted = markdown("<code>a`&lt;b&gt;`</code>");
    assert_eq!(converted, "`a`\\``<b>`\\`");
    assert_eq!(tags(&converted), Vec::<String>::new());
    let fenced = markdown("<pre>````\n&lt;b&gt;</pre>");
    assert_eq!(fenced, "`````\n````\n<b>\n`````");
    assert_eq!(tags(&fenced), Vec::<String>::new());
}

#[test]
fn preformatted_text_is_one_block_of_its_text_and_line_breaks() {
    assert_eq!(markdown("<pre>a<pre>b</pre>c</pre>d"), "```\nabc\n```\n\nd");
    assert_eq!(
        markdown("<pre><b>x</b><br>y<img alt=z></pre>"),
        "```\nx\ny\n```"
    );
    assert_eq!(markdown("<pre>x"), "```\nx\n```");
    let unended = markdown("<pre><pre><p");
    assert_eq!(unended, "```\n<p\n```");
    assert_eq!(tags(&unended), Vec::<String>::new());
}

#[test]
fn lists_number_and_nest() {
    assert_eq!(
        markdown(
            "<ul><li>one</li><li>two<ul><li>inner</li></ul></li></ul><ol start=\"3\"><li>three</li><li>four</li></ol><ol><li>first</li></ol>"
        ),
        "- one\n- two\n  - inner\n\n3. three\n4. four\n\n1. first"
    );
    assert_eq!(markdown("<li>stray</li>"), "- stray");
}

#[test]
fn a_quote_prefixes_each_of_its_lines() {
    assert_eq!(
        markdown("<blockquote><p>said</p><p>twice</p></blockquote>done"),
        "> said\n>\n> twice\n\ndone"
    );
    assert_eq!(markdown("</blockquote>x"), "x");
}

#[test]
fn a_link_keeps_its_target_only_on_the_web_or_mail() {
    assert_eq!(
        markdown(
            "<a href=\"https://example.org/a b)\">site</a> <a href='mailto:x@example.org'>mail</a> <a href=javascript:alert(1)>bad</a> <a>none</a>"
        ),
        "[site](https://example.org/a%20b%29) [mail](mailto:x@example.org) bad none"
    );
    let broken = markdown("<a href=\"https://example.org/a\nb<i>\">site</a>");
    assert_eq!(broken, "[site](https://example.org/a%0Ab%3Ci%3E)");
    assert_eq!(tags(&broken), Vec::<String>::new());
    assert_eq!(
        markdown("<a href=https://example.org>bare</a>"),
        "[bare](https://example.org)"
    );
    assert_eq!(markdown("x</a>"), "x");
}

#[test]
fn scripts_styles_comments_and_unknown_elements_leave_nothing_but_their_text() {
    assert_eq!(
        markdown(
            "<!DOCTYPE html><?xml x?><!-- hidden --><script>alert('<b>')</script><STYLE>p{}</STYLE><span class=x>kept</span><svg><text>drawn</text></svg>"
        ),
        "kept"
    );
    assert_eq!(markdown("<script>never closed"), "");
    assert_eq!(markdown("<!-- never closed"), "");
    assert_eq!(markdown("<!never closed"), "");
    assert_eq!(
        markdown("<img src=x alt=\"a picture\"> <img src=y>"),
        "a picture"
    );
    assert_eq!(
        markdown("<table><tr><td>a</td><td>b</td></tr></table>"),
        "a b"
    );
}

#[test]
fn text_that_would_read_as_markup_is_escaped() {
    assert_eq!(
        markdown("1 < 2 > 0 *not* _em_ [x] #h ~s~ a|b \\ `c`"),
        "1 \\< 2 \\> 0 \\*not\\* \\_em\\_ \\[x\\] \\#h \\~s\\~ a\\|b \\\\ \\`c\\`"
    );
    assert_eq!(
        markdown("<p>- not a list</p><p>+ nor this</p>"),
        "\\- not a list\n\n\\+ nor this"
    );
    assert_eq!(markdown("a &lt;b&gt; &amp; c"), "a \\<b\\> & c");
    assert_eq!(markdown("<p>  </p> <p> spaced </p>"), "spaced");
    assert_eq!(markdown("a<b> </b>c"), "a** **c");
    assert_eq!(markdown("a <br>b"), "a  \nb");
    assert_eq!(markdown("a<br> b"), "a  \nb");
    assert_eq!(markdown("a<br> <b>x</b>"), "a  \n**x**");
    assert_eq!(markdown("a<div>b"), "a\n\nb");
}

#[test]
fn a_tag_that_never_ends_or_opens_nothing_is_text() {
    assert_eq!(markdown("a < b"), "a \\< b");
    assert_eq!(markdown("<p class=\"x"), "\\<p class=\"x");
    assert_eq!(markdown("<a href=\"x\" <i>e</i>"), "\\<a href=\"x\" *e*");
    assert_eq!(markdown("<1>"), "\\<1\\>");
    assert_eq!(markdown("<p =x>y"), "y");
    assert_eq!(markdown("<input disabled value=\"v\"/>z"), "z");
    assert_eq!(markdown("<p\u{3000}/ >x"), "x");
    assert_eq!(markdown("<img title=\"t\"alt=\"pic\">"), "pic");
    assert_eq!(markdown("<img alt=\"\" title>"), "");
    let unended = markdown("\u{e}<script");
    assert_eq!(unended, "\u{e}\\<script");
    assert_eq!(tags(&unended), Vec::<String>::new());
}

#[test]
fn plain_text_drops_tags_and_collapses_whitespace() {
    assert_eq!(
        plain("<b>Hello</b>,\n <i>world</i>&nbsp;&amp; <br>all<p>x</p>"),
        "Hello, world & all x"
    );
}

proptest! {
    #[test]
    fn any_html_converts_without_panicking(html in ".{0,200}") {
        let _ = markdown(&html);
        let _ = plain(&html);
    }

    #[test]
    fn markup_free_text_keeps_its_words(words in proptest::collection::vec("[a-z0-9]{1,8}", 1..8)) {
        let text = words.join(" ");
        prop_assert_eq!(markdown(&text), text.clone());
        prop_assert_eq!(markdown(&format!("<p>{text}</p>")), text);
    }

    #[test]
    fn no_tag_survives(parts in proptest::collection::vec(proptest::sample::select(vec![
        "<pre>", "</pre>", "<code>", "</code>", "`", "```", "&lt;b&gt;", "&#96;", "<b>", "</b>",
        "\n", "<p>", "<br>", "<li>", "<ul>", "</ul>", "<ol>", "<blockquote>", "</blockquote>",
        "<a href=\"https://e.org/\n<i>\">", "</a>", "<h2>", "<script", "<", "x", " ", "-",
    ]), 0..16)) {
        let converted = markdown(&parts.concat());
        prop_assert_eq!(tags(&converted), Vec::<String>::new(), "{:?}", converted);
    }

    #[test]
    fn no_script_survives(inner in "[a-z ]{0,20}") {
        let html = format!("<p>a</p><script>{inner}</script><p>b</p>");
        prop_assert_eq!(markdown(&html), "a\n\nb");
    }
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "each document is read on a thread of its own, so a reader slower than linear fails at the deadline instead of running on"
)]
fn crafted_html_is_read_in_time_linear_in_its_length() {
    let mib = 1 << 20;
    for crafted in [
        "<svg></svg>".repeat(mib / 11),
        "<SVG>x</Svg >".repeat(mib / 13),
        "<a href=\"x\" ".repeat(mib / 12),
        "<".repeat(mib),
        "<b".repeat(mib / 2),
    ] {
        let head = crafted.get(..12).map(ToOwned::to_owned);
        let (done, read) = std::sync::mpsc::channel();
        std::thread::spawn(move || done.send(markdown(&crafted)));
        assert!(
            read.recv_timeout(std::time::Duration::from_secs(10))
                .is_ok(),
            "{head:?} took longer than 10 seconds"
        );
    }
}

#[test]
fn a_dropped_element_closes_whatever_case_it_is_written_in() {
    assert_eq!(markdown("<p>a</p><SVG><p>b</p></sVg >c"), "a\n\nc");
    assert_eq!(markdown("<p>a</p><script>never closed"), "a");
    assert_eq!(markdown("<script>a</b><b></script>after"), "after");
    assert_eq!(markdown("a <b c=d then <i>e</i>"), "a \\<b c=d then *e*");
}
