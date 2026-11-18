use std::collections::BTreeMap;

use herald_testing::ScriptedHttp;
use serde_json::json;

use super::*;
use crate::SourceOptions;

const WHAT: &str = "the listing";

fn asking(http: &ScriptedHttp, cursor: Option<&Cursor>) -> Asked {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    let origin = Origin::new(Forge::GitHub, &options, "o/r");
    let http = Erased(http as &dyn DynHttp);
    pollster::block_on(Asked::asked(
        &http,
        (&origin, None),
        "/things",
        (cursor, "thing"),
    ))
    .unwrap()
}

fn answer(status: u16, body: &serde_json::Value) -> Response {
    Response {
        status,
        headers: vec![("ETag".to_owned(), "\"new\"".to_owned())],
        body: body.to_string().into_bytes(),
    }
}

#[test]
fn a_listing_is_asked_under_both_kept_validators_and_kept_where_unchanged() {
    let kept = Validators {
        etag: Some("\"old\"".to_owned()),
        last_modified: Some("Wed, 18 Nov 2026 08:00:00 GMT".to_owned()),
    };
    let cursor = Cursor {
        listings: BTreeMap::from([("thing".to_owned(), kept.clone())]),
        ..Cursor::default()
    };
    let http = ScriptedHttp::new(vec![answer(304, &serde_json::Value::Null)]);
    let asked = asking(&http, Some(&cursor));
    let request = &http.requests()[0];
    let header = |name: &str| {
        request
            .headers
            .iter()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.expose())
    };
    assert_eq!(header("If-None-Match").as_deref(), Some("\"old\""));
    assert_eq!(
        header("If-Modified-Since").as_deref(),
        Some("Wed, 18 Nov 2026 08:00:00 GMT")
    );
    assert!(!asked.changed());
    assert_eq!(asked.read(WHAT).unwrap(), (kept, None));
}

#[test]
fn a_changed_listing_is_read_with_its_new_validators_and_a_failure_is_refused() {
    let http = ScriptedHttp::new(vec![answer(200, &json!([]))]);
    let asked = asking(&http, None);
    assert!(asked.changed());
    let (kept, body) = asked.read(WHAT).unwrap();
    assert_eq!(kept.etag.as_deref(), Some("\"new\""));
    assert_eq!(body, Some(b"[]".as_slice()));
    let failed = ScriptedHttp::new(vec![answer(500, &json!([]))]);
    match asking(&failed, None).read(WHAT) {
        Err(Unpolled::Unreadable(reason)) => assert!(reason.contains(WHAT), "{reason}"),
        other => panic!("{other:?}"),
    }
}
