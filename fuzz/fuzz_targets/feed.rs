//! Whatever a feed's server sends, the feed parser answers entries or an
//! error, each entry's identity starts with the feed's URL, and HTML of any
//! shape converts to Markdown holding no tag it was given.

#![no_main]

use herald_ports::Response;
use herald_sources_feed::{Feed, FeedOptions, markdown};
use libfuzzer_sys::fuzz_target;
use pulldown_cmark::{Event, Parser};

const URL: &str = "https://feed.example/feed.xml";

fuzz_target!(|bytes: &[u8]| {
    let response = Response {
        status: 200,
        headers: Vec::new(),
        body: bytes.to_vec(),
    };
    let feed = Feed::new(&FeedOptions {
        url: URL.to_owned(),
        backfill: 0,
        page: None,
        remembered: None,
        parse: None,
    });
    if let Ok(polled) = feed.entries(&response) {
        assert!(polled.events.len() <= feed.reading().page);
        for event in &polled.events {
            assert!(event.identity().as_str().starts_with(&format!("{URL}#")));
        }
    }
    if let Ok(html) = std::str::from_utf8(bytes) {
        let converted = markdown(html);
        let tag = Parser::new(&converted)
            .find(|event| matches!(event, Event::Html(_) | Event::InlineHtml(_)));
        assert!(tag.is_none(), "{tag:?} in {converted:?}");
    }
});
