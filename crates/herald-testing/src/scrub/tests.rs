use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::fixture::{Answer, Recorded};

fn exchange(url: &str, request: Value, response: Value) -> Exchange {
    Exchange {
        request: Recorded {
            method: "POST".to_owned(),
            url: url.to_owned(),
            headers: BTreeMap::new(),
            body: Some(request),
        },
        response: Answer {
            status: 200,
            headers: BTreeMap::from([
                ("Retry-After".to_owned(), "1".to_owned()),
                ("Set-Cookie".to_owned(), "__cfruid=abc".to_owned()),
            ]),
            body: Some(response),
        },
    }
}

fn discord() -> Scrub {
    Scrub {
        replace: vec![
            (
                "999999999999999999".to_owned(),
                "1566184281406177531".to_owned(),
            ),
            ("my-bot".to_owned(), "herald".to_owned()),
        ],
        ids: Some(Ids {
            digits: 17..=20,
            first: 1_566_200_000_000_000_001,
        }),
        headers: vec!["retry-after".to_owned()],
    }
}

#[test]
fn values_are_replaced_and_ids_renumbered_in_the_order_they_appear() {
    let fixture = Fixture {
        exchanges: vec![
            exchange(
                "https://discord.com/api/v10/channels/999999999999999999/messages",
                json!({ "content": "from my-bot", "count": 123_456_789_012_345_678_u64 }),
                json!({ "id": "1300000000000000007", "author": { "id": "1300000000000000003" } }),
            ),
            exchange(
                "https://discord.com/api/v10/channels/999999999999999999/messages/1300000000000000007/crosspost",
                json!(["1300000000000000003", 12, null, "at 2026-10-31T12:00:01"]),
                json!("1300000000000000008"),
            ),
        ],
    };
    let scrubbed = discord().applied(&fixture);
    let first = &scrubbed.exchanges[0];
    assert_eq!(
        first.request.url,
        "https://discord.com/api/v10/channels/1566184281406177531/messages"
    );
    assert_eq!(
        first.request.body,
        Some(json!({ "content": "from herald", "count": 123_456_789_012_345_678_u64 }))
    );
    assert_eq!(
        first.response.body,
        Some(json!({ "author": { "id": "1566200000000000001" }, "id": "1566200000000000002" }))
    );
    let second = &scrubbed.exchanges[1];
    assert_eq!(
        second.request.url,
        "https://discord.com/api/v10/channels/1566184281406177531/messages/1566200000000000002/crosspost"
    );
    assert_eq!(
        second.request.body,
        Some(json!([
            "1566200000000000001",
            12,
            null,
            "at 2026-10-31T12:00:01"
        ]))
    );
    assert_eq!(second.response.body, Some(json!("1566200000000000003")));
}

#[test]
fn only_the_kept_response_headers_are_kept() {
    let fixture = Fixture {
        exchanges: vec![exchange("https://discord.com", json!({}), json!({}))],
    };
    let scrubbed = discord().applied(&fixture);
    assert_eq!(
        scrubbed.exchanges[0].response.headers,
        BTreeMap::from([("Retry-After".to_owned(), "1".to_owned())])
    );
}

#[test]
fn numbers_shorter_or_longer_than_an_id_are_kept() {
    let fixture = Fixture {
        exchanges: vec![exchange(
            "https://discord.com/1234567890123456/123456789012345678901",
            json!("12345678901234567"),
            json!(null),
        )],
    };
    let scrubbed = discord().applied(&fixture);
    assert_eq!(
        scrubbed.exchanges[0].request.url,
        "https://discord.com/1234567890123456/123456789012345678901"
    );
    assert_eq!(
        scrubbed.exchanges[0].request.body,
        Some(json!("1566200000000000001"))
    );
}

#[test]
fn without_ids_only_the_replacements_apply() {
    let fixture = Fixture {
        exchanges: vec![exchange(
            "https://discord.com/1300000000000000007",
            json!("my-bot"),
            json!(null),
        )],
    };
    let scrub = Scrub {
        ids: None,
        ..discord()
    };
    let scrubbed = scrub.applied(&fixture);
    assert_eq!(
        scrubbed.exchanges[0].request.url,
        "https://discord.com/1300000000000000007"
    );
    assert_eq!(scrubbed.exchanges[0].request.body, Some(json!("herald")));
}
