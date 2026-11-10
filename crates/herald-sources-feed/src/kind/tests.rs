use herald_kinds::{Fault, SourceConfig, SourceKind};

use super::*;

fn options(url: &str) -> FeedOptions {
    FeedOptions {
        url: url.to_owned(),
        backfill: 4,
        ..FeedOptions::default()
    }
}

#[test]
fn a_feed_is_a_kind_of_source_and_polls_its_url() {
    let feed = FeedKind::checked(options("https://blog.example/feed.xml")).unwrap();
    assert_eq!(FeedKind::NAME, "feed");
    assert_eq!(feed.kind(), "feed");
    assert_eq!(feed.token(), None);
    assert_eq!(feed.kinds(), &[Kind::FEED_ENTRY]);
    assert_eq!(feed.backfill(), 4);
    let poll = feed.poller(None);
    assert_eq!(poll.api(), Feed::new(&feed).api());
    assert_eq!(poll.remembered(), feed.reading().remembered);
}

#[test]
fn a_feed_that_is_no_web_address_keeps_the_source_out() {
    let refused = options("not a url");
    assert_eq!(
        FeedKind::checked(refused.clone()).unwrap_err(),
        Fault::here(refused.check().unwrap_err())
    );
}
