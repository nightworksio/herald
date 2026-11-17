use super::*;
use crate::digest::content_digest;
use crate::message::Covered;

const DOMAIN: &str = "herald.example";
const URL: &str = "https://herald.example/.well-known/herald";
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

fn signed(
    components: Vec<Component>,
    key: &Key,
    created: i64,
    expires: i64,
) -> Vec<(String, String)> {
    let mut fields = vec![
        (CONTENT_TYPE.to_owned(), "application/json".to_owned()),
        (CONTENT_DIGEST.to_owned(), content_digest(BODY)),
    ];
    let covered = Covered {
        components,
        created,
        expires,
        nonce: None,
        keyid: method(DOMAIN, key),
    };
    fields.extend(covered.fields(&[7; 64]));
    fields
}

fn answer<'a>(fields: &'a [(String, String)], body: &'a [u8]) -> Answer<'a> {
    Answer {
        method: "GET",
        url: URL,
        status: 200,
        fields,
        body,
    }
}

#[test]
fn an_answer_signed_by_a_trusted_key_over_what_is_demanded_is_read() {
    let keys = [[1; 32], [2; 32]];
    let fields = signed(demanded().to_vec(), &keys[1], NOW, NOW + 300);
    let (key, shown) = checked(&answer(&fields, BODY), DOMAIN, &keys, bounds()).unwrap();
    assert_eq!(key, &keys[1]);
    assert_eq!(shown.signature, vec![7; 64]);
    assert!(String::from_utf8(shown.base).unwrap().starts_with(
        "\"@status\": 200\n\"content-type\": application/json\n\"content-digest\": sha-256="
    ));
}

#[test]
fn an_answer_signed_by_a_key_not_trusted_is_refused() {
    let fields = signed(demanded().to_vec(), &[3; 32], NOW, NOW + 300);
    assert_eq!(
        checked(&answer(&fields, BODY), DOMAIN, &[[1; 32]], bounds()).unwrap_err(),
        Unanswered::Stranger(method(DOMAIN, &[3; 32]))
    );
    assert_eq!(
        checked(
            &answer(&fields, BODY),
            "other.example",
            &[[3; 32]],
            bounds()
        )
        .unwrap_err()
        .to_string(),
        format!(
            "the answer is signed by {}, a key not trusted",
            method(DOMAIN, &[3; 32])
        )
    );
}

#[test]
fn an_answer_whose_digest_is_missing_or_of_another_body_is_refused() {
    let keys = [[1; 32]];
    let fields = signed(demanded().to_vec(), &keys[0], NOW, NOW + 300);
    assert_eq!(
        checked(&answer(&fields, b"{}"), DOMAIN, &keys, bounds()).unwrap_err(),
        Unanswered::Undigested
    );
    let mut twice = fields.clone();
    twice.insert(2, (CONTENT_DIGEST.to_owned(), content_digest(b"{}")));
    assert!(matches!(
        checked(&answer(&twice, BODY), DOMAIN, &keys, bounds()).unwrap_err(),
        Unanswered::Undigested
    ));
    let unsigned: Vec<(String, String)> = fields
        .iter()
        .filter(|(name, _)| name != CONTENT_DIGEST)
        .cloned()
        .collect();
    assert_eq!(
        checked(&answer(&unsigned, BODY), DOMAIN, &keys, bounds()).unwrap_err(),
        Unanswered::Refused(Refusal::Base(crate::message::Unsigned::Missing {
            component: CONTENT_DIGEST.to_owned()
        }))
    );
    assert_eq!(
        Unanswered::Undigested.to_string(),
        "the answer carries no digest of its body"
    );
}

#[test]
fn a_digest_longer_than_the_fields_read_is_refused() {
    let keys = [[1; 32]];
    let mut fields = signed(demanded().to_vec(), &keys[0], NOW, NOW + 300);
    fields[1].1 = format!("{}{}", "unixsum=:AAAA:, ".repeat(64), content_digest(BODY));
    let longest_other = fields
        .iter()
        .filter(|(name, _)| name != CONTENT_DIGEST)
        .map(|(_, value)| value.len())
        .max()
        .unwrap();
    let mut room = bounds();
    room.fields = fields[1].1.len();
    assert!(checked(&answer(&fields, BODY), DOMAIN, &keys, room).is_ok());
    room.fields -= 1;
    assert!(longest_other < room.fields);
    assert_eq!(
        checked(&answer(&fields, BODY), DOMAIN, &keys, room).unwrap_err(),
        Unanswered::Undigested
    );
}

#[test]
fn an_answer_covering_too_little_or_out_of_its_time_is_refused() {
    let keys = [[1; 32]];
    let refused = |components: Vec<Component>, created: i64, expires: i64| {
        let fields = signed(components, &keys[0], created, expires);
        checked(&answer(&fields, BODY), DOMAIN, &keys, bounds()).unwrap_err()
    };
    let without_uri: Vec<Component> = demanded().into_iter().take(3).collect();
    assert_eq!(
        refused(without_uri, NOW, NOW + 300),
        Unanswered::Refused(Refusal::Uncovered("@target-uri".to_owned()))
    );
    assert_eq!(
        refused(demanded().to_vec(), NOW + 301, NOW + 600),
        Unanswered::Refused(Refusal::Early)
    );
    assert_eq!(
        refused(demanded().to_vec(), NOW - 1200, NOW - 301),
        Unanswered::Refused(Refusal::Expired)
    );
    assert_eq!(
        refused(demanded().to_vec(), NOW, NOW + 1201),
        Unanswered::Refused(Refusal::Lasting)
    );
    assert_eq!(
        Unanswered::Refused(Refusal::Early).to_string(),
        "the signature was made later than now"
    );
}

#[test]
fn an_unsigned_answer_is_refused() {
    let fields = vec![(CONTENT_DIGEST.to_owned(), content_digest(BODY))];
    assert_eq!(
        checked(&answer(&fields, BODY), DOMAIN, &[[1; 32]], bounds()).unwrap_err(),
        Unanswered::Refused(Refusal::Unsigned)
    );
}
