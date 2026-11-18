use super::*;
use crate::digest::content_digest;
use crate::keys::method;
use crate::message::{Covered, Unsigned};

const URL: &str = "https://herald.example/herald/v1/inbox/releases";
const BODY: &[u8] = b"{\"version\":1}";
const NOW: i64 = 1_800_000_000;

fn bounds() -> Bounds {
    Bounds {
        fields: 8192,
        now: NOW,
        skew: 300,
        longest: 1200,
    }
}

fn signed(components: Vec<Component>, nonce: Option<&str>) -> Vec<(String, String)> {
    let mut fields = vec![
        (CONTENT_TYPE.to_owned(), "application/json".to_owned()),
        (CONTENT_DIGEST.to_owned(), content_digest(BODY)),
    ];
    let covered = Covered {
        components,
        created: NOW,
        expires: NOW + 300,
        nonce: nonce.map(str::to_owned),
        keyid: method("peer.example", &[1; 32]),
    };
    fields.extend(covered.fields(&[7; 64]));
    fields
}

fn request<'a>(fields: &'a [(String, String)], body: &'a [u8]) -> Request<'a> {
    Request {
        method: "POST",
        url: URL,
        fields,
        body,
    }
}

fn digest(request: &Request<'_>) -> Unrequested {
    checked(request, bounds()).unwrap().digested().unwrap_err()
}

#[test]
fn a_request_signed_once_over_what_is_demanded_is_read() {
    let fields = signed(demanded().to_vec(), Some("n-1"));
    let signed = checked(&request(&fields, BODY), bounds()).unwrap();
    assert_eq!(signed.keyid(), method("peer.example", &[1; 32]));
    assert_eq!(signed.nonce(), "n-1");
    assert_eq!(signed.expires(), NOW + 300);
    let shown = signed.digested().unwrap();
    assert_eq!(shown.covered.nonce.as_deref(), Some("n-1"));
    assert_eq!(shown.covered.keyid, method("peer.example", &[1; 32]));
    assert_eq!(shown.signature, vec![7; 64]);
    assert!(
        String::from_utf8(shown.base)
            .unwrap()
            .starts_with(&format!("\"@method\": POST\n\"@target-uri\": {URL}\n\"content-type\": application/json\n\"content-digest\": sha-256="))
    );
}

#[test]
fn a_request_signed_without_a_nonce_or_over_too_little_is_refused() {
    let fields = signed(demanded().to_vec(), None);
    assert_eq!(
        checked(&request(&fields, BODY), bounds()).unwrap_err(),
        Unrequested::Refused(Refusal::Unnonced)
    );
    for left_out in demanded() {
        let covered: Vec<Component> = demanded()
            .into_iter()
            .filter(|component| *component != left_out)
            .collect();
        let fields = signed(covered, Some("n-1"));
        assert_eq!(
            checked(&request(&fields, BODY), bounds()).unwrap_err(),
            Unrequested::Refused(Refusal::Uncovered(left_out.name().to_owned()))
        );
    }
    let late = Bounds {
        now: NOW + 1000,
        ..bounds()
    };
    let fields = signed(demanded().to_vec(), Some("n-1"));
    assert_eq!(
        checked(&request(&fields, BODY), late).unwrap_err(),
        Unrequested::Refused(Refusal::Expired)
    );
    assert_eq!(
        checked(&request(&[], BODY), bounds())
            .unwrap_err()
            .to_string(),
        Refusal::Unsigned.to_string()
    );
}

#[test]
fn a_request_whose_digest_is_missing_or_of_another_body_is_refused() {
    let fields = signed(demanded().to_vec(), Some("n-1"));
    assert_eq!(digest(&request(&fields, b"{}")), Unrequested::Undigested);
    let mut twice = fields.clone();
    twice.insert(2, (CONTENT_DIGEST.to_owned(), content_digest(b"{}")));
    assert_eq!(digest(&request(&twice, BODY)), Unrequested::Undigested);
    let undigested: Vec<(String, String)> = fields
        .iter()
        .filter(|(name, _)| name != CONTENT_DIGEST)
        .cloned()
        .collect();
    assert_eq!(
        checked(&request(&undigested, BODY), bounds()).unwrap_err(),
        Unrequested::Refused(Refusal::Base(Unsigned::Missing {
            component: CONTENT_DIGEST.to_owned()
        }))
    );
    let mut long = fields;
    long[1].1 = format!("{}{}", "unixsum=:AAAA:, ".repeat(600), content_digest(BODY));
    assert_eq!(digest(&request(&long, BODY)), Unrequested::Undigested);
    assert_eq!(
        Unrequested::Undigested.to_string(),
        "the request carries no digest of its body"
    );
}
