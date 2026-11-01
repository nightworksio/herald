//! RSS 2.0, RSS 1.0 and Atom 1.0 feeds, read from what their servers send.

#[cfg(test)]
mod feeds {
    use herald_core::{Cursor, Kind, digest};
    use herald_ports::Response;
    use herald_sources_feed::{Feed, FeedOptions};
    use herald_wire::SourceError;
    use proptest::prelude::*;

    const ATOM: &str = include_str!("../fixtures/rust-blog.atom.xml");
    const RSS: &str = include_str!("../fixtures/this-week-in-rust.rss.xml");
    const RDF: &str = include_str!("../fixtures/debian-news.rdf.xml");
    const DATE: &str = "Sun, 01 Nov 2026 04:01:25 GMT";

    fn feed(url: &str) -> Feed {
        Feed::new(&FeedOptions {
            url: url.to_owned(),
            backfill: 0,
        })
    }

    fn answer(status: u16, body: &str, date: bool) -> Response {
        let mut headers = vec![
            ("ETag".to_owned(), "\"6ae08b67-18539\"".to_owned()),
            (
                "Last-Modified".to_owned(),
                "Tue, 27 Oct 2026 11:30:47 GMT".to_owned(),
            ),
        ];
        if date {
            headers.push(("Date".to_owned(), DATE.to_owned()));
        }
        Response {
            status,
            headers,
            body: body.as_bytes().to_vec(),
        }
    }

    fn rss(items: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?><rss version=\"2.0\" xmlns:content=\"http://purl.org/rss/1.0/modules/content/\"><channel><title>Notes</title>{items}</channel></rss>"
        )
    }

    fn atom(entries: &str) -> String {
        format!(
            "<feed xmlns=\"http://www.w3.org/2005/Atom\"><title type=\"html\">A &amp;amp; B</title>{entries}</feed>"
        )
    }

    #[test]
    fn an_atom_feed_s_entries_become_events_newest_first_with_the_validators() {
        let blog = feed("https://blog.rust-lang.org/feed.xml");
        let polled = blog.entries(&answer(200, ATOM, true)).unwrap();
        assert_eq!(polled.events.len(), 10);
        let newest = &polled.events[0];
        assert_eq!(
            newest.identity().as_str(),
            "https://blog.rust-lang.org/feed.xml#https://blog.rust-lang.org/2026/10/26/how-to-fund-rust-contributors/"
        );
        assert_eq!(newest.kind(), Kind::FeedEntry);
        assert_eq!(newest.project(), "Rust Blog");
        assert_eq!(newest.title(), "How to fund Rust contributors");
        assert_eq!(newest.version(), None);
        assert!(newest.body().starts_with("> TLDR:\n>\n> - To fund general maintenance, donate to the [Rust Foundation Maintainers Fund](https://github.com/sponsors/rustfoundation)"), "{}", newest.body());
        assert_eq!(
            newest.link("entry"),
            Some("https://blog.rust-lang.org/2026/10/26/how-to-fund-rust-contributors/")
        );
        assert_eq!(newest.occurred().to_string(), "2026-10-26T00:00:00Z");
        assert_eq!(polled.events[1].title(), "Announcing rustup 1.29.2");
        assert_eq!(polled.etag.as_deref(), Some("\"6ae08b67-18539\""));
        assert_eq!(
            polled.last_modified.as_deref(),
            Some("Tue, 27 Oct 2026 11:30:47 GMT")
        );
    }

    #[test]
    fn an_rss_2_feed_s_items_take_their_guid_and_pub_date() {
        let twir = feed("https://this-week-in-rust.org/rss.xml");
        let polled = twir.entries(&answer(200, RSS, true)).unwrap();
        assert_eq!(polled.events.len(), 4);
        let newest = &polled.events[0];
        assert_eq!(
            newest.identity().as_str(),
            "https://this-week-in-rust.org/rss.xml#tag:this-week-in-rust.org,2026-10-28:/blog/2026/10/28/this-week-in-rust-675/"
        );
        assert_eq!(newest.project(), "This Week in Rust");
        assert_eq!(newest.title(), "This Week in Rust 675");
        assert_eq!(newest.occurred().to_string(), "2026-10-28T04:00:00Z");
        assert!(!newest.body().contains("<p>"));
    }

    #[test]
    fn an_rss_1_feed_s_items_take_their_about_and_a_bare_date() {
        let debian = feed("https://www.debian.org/News/news");
        let polled = debian.entries(&answer(200, RDF, true)).unwrap();
        assert_eq!(polled.events.len(), 6);
        let newest = &polled.events[0];
        assert_eq!(
            newest.identity().as_str(),
            "https://www.debian.org/News/news#https://www.debian.org/News/2026/20260912"
        );
        assert_eq!(newest.project(), "Debian News");
        assert_eq!(newest.title(), "Updated Debian 13: 13.7 released");
        assert_eq!(newest.occurred().to_string(), "2026-09-12T00:00:00Z");
        assert!(
            newest.body().contains("(codename trixie)"),
            "{}",
            newest.body()
        );
    }

    #[test]
    fn an_entry_without_an_id_is_known_by_its_link_and_else_by_its_title_and_time() {
        let notes = feed("https://notes.example/rss");
        let body = rss(
            "<item><title>Linked</title><link>https://notes.example/1</link><pubDate>Sat, 31 Oct 2026 12:00:00 +0000</pubDate></item>\
             <item><title>Bare</title><pubDate>2026-10-30T12:00:00Z</pubDate></item>\
             <item><title>Undated</title><guid>u</guid></item>",
        );
        let polled = notes.entries(&answer(200, &body, true)).unwrap();
        let found: Vec<(&str, String)> = polled
            .events
            .iter()
            .map(|event| (event.title(), event.identity().as_str().to_owned()))
            .collect();
        assert_eq!(
            found,
            [
                ("Undated", "https://notes.example/rss#u".to_owned()),
                (
                    "Linked",
                    "https://notes.example/rss#https://notes.example/1".to_owned()
                ),
                (
                    "Bare",
                    format!(
                        "https://notes.example/rss#{}",
                        digest("Bare\n2026-10-30T12:00:00Z")
                    )
                ),
            ]
        );
        assert_eq!(
            polled.events[0].occurred().to_string(),
            "2026-11-01T04:01:25Z"
        );
        let undated = notes.entries(&answer(200, &body, false)).unwrap();
        assert_eq!(undated.events.len(), 2);
    }

    #[test]
    fn an_atom_entry_s_text_html_and_xhtml_each_read_as_written() {
        let notes = feed("https://notes.example/atom");
        let body = atom(
            "<entry><id>1</id><title type=\"html\">&lt;b&gt;Bold&lt;/b&gt; news</title><updated>2026-10-31T12:00:00Z</updated>\
               <link rel=\"self\" href=\"https://notes.example/self\"/><link href=\"https://notes.example/1\"/><link href=\"https://notes.example/other\"/>\
               <summary>not this</summary><content type=\"text\">1 &lt; 2 &amp; *star*</content></entry>\
             <entry><id>2</id><title>Two</title><published>2026-10-30T12:00:00Z</published>\
               <content type=\"xhtml\"><div xmlns=\"http://www.w3.org/1999/xhtml\"><p>Hello <em>there</em></p></div></content></entry>\
             <entry><id>3</id><title></title><published>2026-10-29T12:00:00Z</published><link href=\"javascript:alert(1)\"/>\
               <summary type=\"html\">&lt;p&gt;Only &amp;mystery; a summary&lt;/p&gt;</summary></entry>",
        );
        let polled = notes.entries(&answer(200, &body, true)).unwrap();
        let first = &polled.events[0];
        assert_eq!(first.project(), "A & B");
        assert_eq!(first.title(), "Bold news");
        assert_eq!(first.body(), "1 \\< 2 & \\*star\\*");
        assert_eq!(first.link("entry"), Some("https://notes.example/1"));
        assert_eq!(polled.events[1].body(), "Hello *there*");
        let third = &polled.events[2];
        assert_eq!(third.title(), "A & B");
        assert_eq!(third.body(), "Only &mystery; a summary");
        assert_eq!(third.link("entry"), None);
    }

    #[test]
    fn an_rss_item_prefers_its_full_content_and_keeps_cdata_as_written() {
        let notes = feed("https://notes.example/rss");
        let body = rss(
            "<item><guid>c</guid><title>T &#8211; x</title><description>short</description>\
             <content:encoded><![CDATA[<p>Full <a href=\"https://notes.example\">story</a></p>]]></content:encoded>\
             <pubDate>Sat, 31 Oct 2026 12:00:00 GMT</pubDate></item>",
        );
        let polled = notes.entries(&answer(200, &body, true)).unwrap();
        assert_eq!(polled.events[0].title(), "T – x");
        assert_eq!(
            polled.events[0].body(),
            "Full [story](https://notes.example)"
        );
    }

    #[test]
    fn markup_inside_a_field_and_an_unknown_reference_are_read_as_text() {
        let notes = feed("https://notes.example/atom");
        let body = atom(
            "<subtitle>x &amp; y</subtitle><!-- a note --><entry><id>n</id><title>A &mystery; B</title><updated>2026-10-31T12:00:00Z</updated>\
             <summary type=\"html\"><p>raw <b>markup</b></p></summary><extra><id>ignored</id></extra></entry>",
        );
        let polled = notes.entries(&answer(200, &body, true)).unwrap();
        assert_eq!(polled.events[0].title(), "A &mystery; B");
        assert_eq!(polled.events[0].body(), "raw markup");
        assert_eq!(
            polled.events[0].identity().as_str(),
            "https://notes.example/atom#n"
        );
    }

    #[test]
    fn a_feed_with_no_url_or_title_names_no_project() {
        let nowhere = feed("");
        let body = "<rss><channel><item><guid>a</guid><title>A</title></item></channel></rss>";
        assert_eq!(
            nowhere.entries(&answer(200, body, true)),
            Err(SourceError::Invalid {
                what: "the feed",
                problem: herald_core::Problem::NoProject
            })
        );
    }

    #[test]
    fn a_feed_without_a_title_is_named_by_its_host() {
        let notes = feed("https://notes.example/rss");
        let body = "<rss><channel><item><guid>a</guid><title>A</title></item></channel></rss>";
        let polled = notes.entries(&answer(200, body, true)).unwrap();
        assert_eq!(polled.events[0].project(), "notes.example");
    }

    #[test]
    fn what_is_not_a_feed_or_not_an_answer_says_so() {
        let notes = feed("https://notes.example/rss");
        for body in ["<html><body/></html>", "", "not xml at all"] {
            assert_eq!(
                notes.entries(&answer(200, body, true)),
                Err(SourceError::Unreadable {
                    what: "the feed",
                    reason: "it is neither RSS nor Atom".to_owned()
                }),
                "{body}"
            );
        }
        assert!(matches!(
            notes.entries(&answer(200, "<rss><channel></item></rss>", true)),
            Err(SourceError::Unreadable { .. })
        ));
        assert_eq!(
            notes.entries(&answer(503, "", true)),
            Err(SourceError::Answered {
                what: "the feed",
                status: 503
            })
        );
        let huge = format!("<rss>{}</rss>", " ".repeat(4_194_304));
        assert_eq!(
            notes.entries(&answer(200, &huge, true)),
            Err(SourceError::TooLarge {
                what: "the feed",
                limit: 4_194_304
            })
        );
        assert!(Feed::unchanged(&answer(304, "", true)));
        assert!(!Feed::unchanged(&answer(200, "", true)));
    }

    #[test]
    fn the_feed_is_asked_for_conditionally_at_its_own_url() {
        let notes = feed(" https://notes.example/rss ");
        assert_eq!(notes.api(), "https://notes.example/rss");
        let cursor = Cursor {
            seen: Vec::new(),
            etag: Some("\"a\"".to_owned()),
            last_modified: Some(DATE.to_owned()),
        };
        assert_eq!(
            notes.request(Some(&cursor)).to_string(),
            [
                "GET https://notes.example/rss",
                "Accept: application/atom+xml, application/rss+xml, application/xml;q=0.9, text/xml;q=0.9, */*;q=0.1",
                "User-Agent: herald/0.0.0 (+https://github.com/nightworksio/herald)",
                "If-None-Match: \"a\"",
                "If-Modified-Since: Sun, 01 Nov 2026 04:01:25 GMT",
            ]
            .join("\n")
        );
        assert_eq!(notes.request(None).headers.len(), 2);
    }

    proptest! {
        #[test]
        fn any_bytes_are_entries_or_an_error(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let response = Response { status: 200, headers: Vec::new(), body: bytes };
            let _ = feed("https://notes.example/rss").entries(&response);
        }

        #[test]
        fn an_item_s_guid_is_its_identity(guid in "[a-z0-9:/.-]{1,24}") {
            let body = rss(&format!("<item><guid>{guid}</guid><title>t</title><pubDate>2026-10-30T12:00:00Z</pubDate></item>"));
            let polled = feed("https://notes.example/rss").entries(&answer(200, &body, true)).unwrap();
            prop_assert_eq!(polled.events[0].identity().as_str(), format!("https://notes.example/rss#{guid}"));
        }
    }
}
