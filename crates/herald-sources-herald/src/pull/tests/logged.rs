//! A peer's feed read from its static signed log.

use herald_core::{Feed, FeedItem};
use herald_peers::discovery::Instance;
use herald_peers::pages::{Log, Logged, Written, statement};

use super::*;

const SIGNATURE: &str = "https://peer.example/.well-known/herald.sig";
const OLDER: &str = "https://peer.example/herald/v1/pages/releases/8";

const INSTANCE: Instance<'static> = Instance {
    domain: DOMAIN,
    base: "",
    software: "herald",
    version: "1",
    repository: "https://github.com/nightworksio/herald",
};

fn statically() -> Pulled {
    source(HeraldFile {
        fixed: true,
        ..HeraldFile::default()
    })
}

fn now() -> Timestamp {
    Timestamp::from_second(NOW).unwrap()
}

fn plain(body: impl Into<Vec<u8>>) -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: body.into(),
    }
}

/// The feed listing `items`, one page for each, newest first.
fn pages(items: &[(u64, &str)]) -> Vec<Written> {
    let feed = Feed {
        newest: items.first().map_or(0, |(sequence, _)| *sequence),
        items: items
            .iter()
            .map(|(sequence, identity)| FeedItem {
                sequence: *sequence,
                envelope: envelope(identity),
            })
            .collect(),
    };
    INSTANCE.pages("releases", &feed, 1)
}

/// The discovery document of a log listing `pages` under `log`, and its
/// signature naming `keyid`, made by `by`.
fn logged(pages: &[Written], log: Log, (keyid, by): (&PublicKey, &PublicKey)) -> [Response; 2] {
    let discovery = INSTANCE.logged(
        &json!({}),
        &[Logged {
            name: "releases",
            newest: 9,
            pages,
        }],
        log,
    );
    let unproved = proof::unproved(
        &statement(PEER, discovery.as_bytes(), log),
        &method(DOMAIN, keyid),
        log.published,
    )
    .unwrap();
    let signature = fake_ed25519(by, unproved.message());
    [
        plain(discovery),
        plain(unproved.proved(&signature).to_string()),
    ]
}

fn log(sequence: u64) -> Log {
    Log {
        sequence,
        published: now(),
    }
}

fn answers(head: [Response; 2], pages: &[Written]) -> Vec<Response> {
    head.into_iter()
        .chain(pages.iter().map(|page| plain(page.body.clone())))
        .collect()
}

fn cursor(sequence: u64, log: u64) -> Cursor {
    Cursor {
        sequence: Some(sequence),
        log: Some(log),
        ..Cursor::default()
    }
}

fn unreadable(read: Result<Option<Polled>, Unpulled>) -> String {
    match read {
        Err(Unpulled::Unreadable(why)) => why,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_static_log_is_read_page_by_page_once_its_signature_and_each_digest_hold() {
    let written = pages(&[(9, "a/1"), (8, "a/2")]);
    let (read, http) = pulled(
        &statically(),
        answers(logged(&written, log(4), (&KEY, &KEY)), &written),
        Some(&cursor(7, 3)),
    );
    let read = read.unwrap().unwrap();
    assert_eq!(
        identities(&read),
        ["herald/upstream/a/1", "herald/upstream/a/2"]
    );
    assert_eq!(
        (read.sequence, read.log, read.etag, read.relisted),
        (Some(9), Some(4), None, true)
    );
    assert_eq!(
        http.asked(),
        [
            (PEER.to_owned(), None),
            (SIGNATURE.to_owned(), None),
            (FEED.to_owned(), None),
            (OLDER.to_owned(), None),
        ]
    );
    assert!(
        http.requests()
            .iter()
            .all(|request| request.most == trust().most)
    );
    let first = pulled(
        &statically(),
        answers(logged(&written, log(4), (&KEY, &KEY)), &written[..1]),
        None,
    )
    .0
    .unwrap()
    .unwrap();
    assert_eq!(identities(&first), ["herald/upstream/a/1"]);
    let reached = pulled(
        &statically(),
        answers(logged(&written, log(4), (&KEY, &KEY)), &written),
        Some(&cursor(8, 3)),
    )
    .0
    .unwrap()
    .unwrap();
    assert_eq!(identities(&reached), ["herald/upstream/a/1"]);
}

#[test]
fn a_log_already_read_reads_no_page_and_an_older_one_is_refused() {
    let written = pages(&[(9, "a/1")]);
    let (read, http) = pulled(
        &statically(),
        answers(logged(&written, log(4), (&KEY, &KEY)), &[]),
        Some(&cursor(9, 4)),
    );
    assert_eq!(read, Ok(None));
    assert_eq!(http.asked().len(), 2);
    assert_eq!(
        unreadable(
            pulled(
                &statically(),
                answers(logged(&written, log(3), (&KEY, &KEY)), &[]),
                Some(&cursor(9, 4)),
            )
            .0
        ),
        "the static log is at 3, older than the 4 read before; its host may be serving an old copy"
    );
}

#[test]
fn a_log_written_ahead_or_longer_ago_than_stale_after_is_refused() {
    let written = pages(&[(9, "a/1")]);
    let ahead = Log {
        sequence: 1,
        published: Timestamp::from_second(NOW + 301).unwrap(),
    };
    assert_eq!(
        unreadable(
            pulled(
                &statically(),
                answers(logged(&written, ahead, (&KEY, &KEY)), &[]),
                None
            )
            .0
        ),
        "the static log was written at 2027-01-15T08:05:01Z, ahead of now"
    );
    let skewed = Log {
        published: Timestamp::from_second(NOW + 300).unwrap(),
        ..ahead
    };
    assert!(
        pulled(
            &statically(),
            answers(logged(&written, skewed, (&KEY, &KEY)), &written),
            None
        )
        .0
        .is_ok()
    );
    let days = |seconds: i64| Log {
        sequence: 1,
        published: Timestamp::from_second(NOW - seconds).unwrap(),
    };
    assert_eq!(
        unreadable(
            pulled(
                &statically(),
                answers(logged(&written, days(3 * 86_400 + 1), (&KEY, &KEY)), &[]),
                None
            )
            .0
        ),
        "the static log was written at 2027-01-12T07:59:59Z, longer ago than stale_after allows; its host may be withholding newer pages"
    );
    assert!(
        pulled(
            &statically(),
            answers(logged(&written, days(3 * 86_400), (&KEY, &KEY)), &written),
            None
        )
        .0
        .is_ok()
    );
}

#[test]
fn a_log_signed_by_a_stranger_or_whose_signature_or_page_does_not_hold_is_refused() {
    let written = pages(&[(9, "a/1")]);
    assert_eq!(
        pulled(
            &statically(),
            answers(logged(&written, log(1), (&STRANGER, &STRANGER)), &[]),
            None
        )
        .0,
        Err(Unpulled::Stranger(method(DOMAIN, &STRANGER)))
    );
    assert_eq!(
        unreadable(
            pulled(
                &statically(),
                answers(logged(&written, log(1), (&KEY, &STRANGER)), &[]),
                None
            )
            .0
        ),
        "the static log's signature does not hold"
    );
    let swapped = pages(&[(9, "a/9")]);
    assert_eq!(
        unreadable(
            pulled(
                &statically(),
                answers(logged(&written, log(1), (&KEY, &KEY)), &swapped),
                None
            )
            .0
        ),
        format!(
            "the page {FEED} is not the one the static log lists; its host may be serving another copy"
        )
    );
    let [discovery, _] = logged(&written, log(1), (&KEY, &KEY));
    assert_eq!(
        unreadable(pulled(&statically(), vec![discovery, plain("{")], None).0),
        "the static log's file is no JSON"
    );
    let elsewhere = INSTANCE.pages("tags", &Feed::default(), 1);
    assert_eq!(
        unreadable(
            pulled(
                &statically(),
                logged(&elsewhere, log(1), (&KEY, &KEY)).into(),
                None
            )
            .0
        ),
        "the static log cannot be read: a feed's first page is not at its URL"
    );
}

#[test]
fn a_log_whose_file_is_not_answered_is_not_read() {
    let missing = Response {
        status: 404,
        headers: Vec::new(),
        body: Vec::new(),
    };
    assert_eq!(
        unreadable(pulled(&statically(), vec![missing], None).0),
        format!("the static log's {PEER} was answered 404")
    );
    assert!(matches!(
        pulled(&statically(), Vec::new(), None).0,
        Err(Unpulled::Unreachable(_))
    ));
}
