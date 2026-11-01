use super::*;
use proptest::prelude::*;

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
}

#[test]
fn a_tag_that_never_ends_or_opens_nothing_is_text() {
    assert_eq!(markdown("a < b"), "a \\< b");
    assert_eq!(markdown("<p class=\"x"), "\\<p class=\"x");
    assert_eq!(markdown("<1>"), "\\<1\\>");
    assert_eq!(markdown("<p =x>y"), "y");
    assert_eq!(markdown("<input disabled value=\"v\"/>z"), "z");
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
    fn no_script_survives(inner in "[a-z ]{0,20}") {
        let html = format!("<p>a</p><script>{inner}</script><p>b</p>");
        prop_assert_eq!(markdown(&html), "a\n\nb");
    }
}
