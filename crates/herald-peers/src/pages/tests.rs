use serde_json::json;

use super::*;
use crate::keys::{KeySet, method};

const INSTANCE: Instance<'static> = Instance {
    domain: "owner.example",
    base: "/log",
    software: "herald",
    version: "1.2.3",
    repository: "https://github.com/nightworksio/herald",
};

const HEAD: &str = "https://owner.example/log/herald/v1/feeds/releases";

const LOG: Log = Log {
    sequence: 7,
    published: Timestamp::constant(1_800_000_000, 0),
};

fn feed(sequences: &[u64]) -> Feed {
    Feed {
        newest: sequences.first().copied().unwrap_or_default(),
        items: sequences
            .iter()
            .map(|sequence| FeedItem {
                sequence: *sequence,
                envelope: json!({ "identity": format!("e{sequence}") }),
            })
            .collect(),
    }
}

fn read(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn a_feed_s_pages_are_its_buckets_newest_first_the_newest_at_the_feed_s_url() {
    let pages = INSTANCE.pages("releases", &feed(&[9, 8, 5, 4, 1]), 4);
    let urls: Vec<&str> = pages.iter().map(|page| page.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            HEAD,
            "https://owner.example/log/herald/v1/pages/releases/1",
            "https://owner.example/log/herald/v1/pages/releases/0",
        ]
    );
    assert_eq!(
        read(&pages[0].body),
        json!({
            "id": HEAD,
            "feed": HEAD,
            "items": [
                { "sequence": 9, "envelope": { "identity": "e9" } },
                { "sequence": 8, "envelope": { "identity": "e8" } },
            ],
            "next": urls[1],
        })
    );
    assert_eq!(read(&pages[1].body)["next"], json!(urls[2]));
    assert_eq!(read(&pages[2].body).get("next"), None);
    assert_eq!(
        read(&pages[2].body)["items"],
        json!([{ "sequence": 1, "envelope": { "identity": "e1" } }])
    );
    let walked: Vec<u64> = pages
        .iter()
        .flat_map(|page| {
            crate::reading::page(page.body.as_bytes(), &page.url)
                .unwrap()
                .items
        })
        .map(|(sequence, _)| sequence)
        .collect();
    assert_eq!(walked, [9, 8, 5, 4, 1]);
}

#[test]
fn an_empty_feed_is_one_page_listing_nothing_and_a_page_size_of_none_is_one() {
    let empty = INSTANCE.pages("releases", &Feed::default(), 20);
    assert_eq!(empty.len(), 1);
    assert_eq!(
        read(&empty[0].body),
        json!({ "id": HEAD, "feed": HEAD, "items": [] })
    );
    let single = INSTANCE.pages("releases", &feed(&[3, 2]), 0);
    assert_eq!(single.len(), 2);
    assert_eq!(single[1].url, INSTANCE.page_of("releases", 2));
    assert_eq!(
        INSTANCE.page_of("a b", 4),
        "https://owner.example/log/herald/v1/pages/a%20b/4"
    );
}

#[test]
fn the_log_s_discovery_document_lists_each_page_s_digest_under_its_sequence() {
    let keys = KeySet {
        sequence: 2,
        refresh: 86_400,
        keys: vec![[1; 32]],
    }
    .document("owner.example");
    let pages = INSTANCE.pages("releases", &feed(&[5, 1]), 4);
    let logged = Logged {
        name: "releases",
        newest: 5,
        pages: &pages,
    };
    let body = INSTANCE.logged(&keys, &[logged], LOG);
    let document = read(&body);
    assert_eq!(
        document["log"],
        json!({ "sequence": 7, "published": "2027-01-15T08:00:00Z" })
    );
    assert_eq!(document["keys"], keys);
    assert_eq!(document["features"], json!(["feeds"]));
    assert_eq!(
        document["feeds"][0],
        json!({
            "name": "releases",
            "url": HEAD,
            "sequence": 5,
            "pages": [
                { "url": HEAD, "digest": content_digest(pages[0].body.as_bytes()) },
                { "url": pages[1].url, "digest": content_digest(pages[1].body.as_bytes()) },
            ],
        })
    );
    assert_eq!(
        listing(body.as_bytes(), LOG, HEAD),
        Ok(Listing {
            newest: 5,
            pages: pages
                .iter()
                .map(|page| (page.url.clone(), content_digest(page.body.as_bytes())))
                .collect(),
        })
    );
}

#[test]
fn a_listing_of_another_log_or_feed_or_shape_is_not_read() {
    let unlisted = Unsigned::Unlisted(HEAD.to_owned());
    let logged = json!({ "sequence": 7, "published": "2027-01-15T08:00:00Z" });
    for (document, why) in [
        (json!("text"), Unsigned::Shape("it names no sequence")),
        (
            json!({ "log": { "sequence": 7 } }),
            Unsigned::Shape("it names no time it was written"),
        ),
        (
            json!({ "log": { "sequence": 8, "published": "2027-01-15T08:00:00Z" } }),
            Unsigned::Shape("its log is not the one its signature names"),
        ),
        (json!({ "log": logged }), unlisted.clone()),
        (
            json!({ "log": logged, "feeds": [{ "url": "https://elsewhere.example" }] }),
            unlisted,
        ),
        (
            json!({ "log": logged, "feeds": [{ "url": HEAD }] }),
            Unsigned::Shape("a feed names no sequence"),
        ),
        (
            json!({ "log": logged, "feeds": [{ "url": HEAD, "sequence": 1, "pages": [{ "url": HEAD }] }] }),
            Unsigned::Shape("a page names no URL or no digest"),
        ),
        (
            json!({ "log": logged, "feeds": [{ "url": HEAD, "sequence": 1 }] }),
            Unsigned::Shape("a feed's first page is not at its URL"),
        ),
        (
            json!({ "log": logged, "feeds": [{ "url": HEAD, "sequence": 1, "pages": [{ "url": "x", "digest": "d" }] }] }),
            Unsigned::Shape("a feed's first page is not at its URL"),
        ),
    ] {
        assert_eq!(
            listing(document.to_string().as_bytes(), LOG, HEAD),
            Err(why),
            "{document}"
        );
    }
    assert_eq!(listing(b"{", LOG, HEAD), Err(Unsigned::Json));
}

fn proved(statement: &Value) -> (Vec<u8>, Vec<u8>) {
    let unproved =
        proof::unproved(statement, &method("owner.example", &[1; 32]), LOG.published).unwrap();
    let message = unproved.message().to_vec();
    (unproved.proved(&[7; 64]).to_string().into_bytes(), message)
}

#[test]
fn a_detached_signature_states_the_file_s_url_digest_and_log_and_is_read_back() {
    let url = INSTANCE.discovery();
    let body = b"{\"log\":7}";
    let statement = statement(&url, body, LOG);
    assert_eq!(
        statement,
        json!({
            "type": "herald:Signed",
            "url": url,
            "digest": content_digest(body),
            "sequence": 7,
            "published": "2027-01-15T08:00:00Z",
        })
    );
    let (signature, message) = proved(&statement);
    let read = signed(&signature, &url, body).unwrap();
    assert_eq!(read.log, LOG);
    assert_eq!(read.proof.message, message);
    assert_eq!(read.proof.signature, vec![7; 64]);
    assert_eq!(read.proof.method, method("owner.example", &[1; 32]));
    assert_eq!(
        signature_of(&url),
        "https://owner.example/log/.well-known/herald.sig"
    );
}

#[test]
fn a_signature_of_another_file_bytes_or_shape_is_not_read() {
    let url = INSTANCE.discovery();
    let body = b"{}";
    let (signature, _) = proved(&statement(&url, body, LOG));
    assert_eq!(
        signed(&signature, HEAD, body),
        Err(Unsigned::Elsewhere(url.clone()))
    );
    assert_eq!(signed(&signature, &url, b"[]"), Err(Unsigned::Undigested));
    assert_eq!(signed(b"{", &url, body), Err(Unsigned::Json));
    let unproved = statement(&url, body, LOG).to_string();
    assert_eq!(
        signed(unproved.as_bytes(), &url, body),
        Err(Unsigned::Unproven(Unproven::Unproved))
    );
    for (changed, why) in [
        (
            json!({ "type": "herald:Event" }),
            "the signature is no herald:Signed statement",
        ),
        (json!({ "url": 7 }), "the signature names no file"),
        (json!({ "sequence": "7" }), "it names no sequence"),
        (
            json!({ "published": "yesterday" }),
            "it names no time it was written",
        ),
    ] {
        let mut written = statement(&url, body, LOG);
        for (name, value) in changed.as_object().unwrap() {
            written[name] = value.clone();
        }
        assert_eq!(
            signed(written.to_string().as_bytes(), &url, body),
            Err(Unsigned::Shape(why)),
            "{changed}"
        );
    }
}

#[test]
fn each_reason_a_log_is_not_read_says_what_is_wrong() {
    for (why, said) in [
        (Unsigned::Json, "the static log's file is no JSON"),
        (
            Unsigned::Shape("it names no sequence"),
            "the static log cannot be read: it names no sequence",
        ),
        (
            Unsigned::Elsewhere("https://a.example".to_owned()),
            "the signature is of https://a.example, not the file read",
        ),
        (
            Unsigned::Undigested,
            "the signature names the digest of other bytes than the file's",
        ),
        (
            Unsigned::Unproven(Unproven::Unproved),
            "the object carries no proof",
        ),
        (
            Unsigned::Unlisted(HEAD.to_owned()),
            "the static log lists no feed at https://owner.example/log/herald/v1/feeds/releases",
        ),
    ] {
        assert_eq!(why.to_string(), said);
    }
}
