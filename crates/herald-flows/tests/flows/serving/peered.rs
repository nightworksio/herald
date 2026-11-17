//! A peer's discovery document read as another herald reads it: its answer
//! checked against the keys trusted for it before the document is taken.

use herald_flows::peering::{Discovered, Reading, Undiscovered, bounds, discovered};
use herald_peers::answer::Unanswered;
use herald_peers::keys::{Key, Unkeyed, method};
use herald_ports::{HttpError, Response};
use herald_testing::ScriptedHttp;

use super::*;

const URL: &str = "https://herald.example/.well-known/herald";
const MOST: usize = 4096;

fn key(text: &str) -> Key {
    text.as_bytes().try_into().unwrap()
}

fn served() -> (Config, Response) {
    let text = format!(
        "{}\n[federation]\ndomain = \"herald.example\"\nkeys = [{{ secret = \"HERALD_KEY_NEW\" }}, {{ secret = \"HERALD_KEY_OLD\" }}]\n",
        config_text(),
    );
    let config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let answered =
        World::with(config.clone(), Script::default()).answer(&get("/.well-known/herald"));
    let mut headers = vec![("content-type".to_owned(), answered.reply.kind.to_owned())];
    headers.extend(answered.reply.headers.iter().cloned());
    let response = Response {
        status: answered.reply.status,
        headers,
        body: answered.reply.body.clone(),
    };
    (config, response)
}

fn fetched(
    config: &Config,
    answers: Vec<Response>,
    url: &str,
    pinned: Option<&[Key]>,
) -> (Result<Discovered, Undiscovered>, ScriptedHttp) {
    let http = ScriptedHttp::new(answers);
    let reading = Reading {
        url,
        domain: "herald.example",
        most: MOST,
        bounds: bounds(config, at()),
    };
    let read = pollster::block_on(discovered(&http, &FakeVerifier, reading, pinned));
    (read, http)
}

#[test]
fn a_document_signed_by_a_pinned_key_is_read_with_the_keys_it_lists() {
    let (config, response) = served();
    let pinned = [key(NEW_KEY)];
    let (read, http) = fetched(&config, vec![response.clone()], URL, Some(&pinned));
    let read = read.unwrap();
    assert_eq!(read.by, key(NEW_KEY));
    assert_eq!(read.set.keys, [key(NEW_KEY), key(OLD_KEY)]);
    assert_eq!(read.set.sequence, 1);
    let asked = http.requests();
    assert_eq!(asked[0].most, MOST);
    assert_eq!(asked[0].url.redacted(), URL);
    assert!(
        asked[0]
            .headers
            .iter()
            .any(|(name, value)| name == "Accept" && value.redacted() == "application/json")
    );
    let (unpinned, _) = read_listed(&config, response);
    assert_eq!(unpinned.unwrap(), read);
}

fn read_listed(
    config: &Config,
    response: Response,
) -> (Result<Discovered, Undiscovered>, ScriptedHttp) {
    fetched(config, vec![response], URL, None)
}

#[test]
fn a_document_signed_by_a_key_not_pinned_is_refused() {
    let (config, response) = served();
    let pinned = [key(OLD_KEY)];
    let (read, _) = fetched(&config, vec![response], URL, Some(&pinned));
    assert_eq!(
        read.unwrap_err(),
        Undiscovered::Unanswered(Unanswered::Stranger(method(
            "herald.example",
            &key(NEW_KEY)
        )))
    );
}

#[test]
fn a_document_changed_on_the_way_or_answering_another_url_is_refused() {
    let (config, response) = served();
    let pinned = [key(NEW_KEY)];
    let mut changed = response.clone();
    changed.body.push(b' ');
    let (read, _) = fetched(&config, vec![changed], URL, Some(&pinned));
    assert_eq!(
        read.unwrap_err(),
        Undiscovered::Unanswered(Unanswered::Undigested)
    );
    let elsewhere = "https://herald.example/.well-known/herald?again=1";
    let (read, _) = fetched(&config, vec![response], elsewhere, Some(&pinned));
    assert_eq!(read.unwrap_err(), Undiscovered::Forged);
}

#[test]
fn a_document_not_answered_200_or_not_reached_is_not_read() {
    let (config, response) = served();
    let missing = Response {
        status: 404,
        ..response
    };
    let (read, _) = fetched(&config, vec![missing], URL, None);
    assert_eq!(read.unwrap_err(), Undiscovered::Status(404));
    let (read, _) = fetched(&config, Vec::new(), URL, None);
    assert_eq!(
        read.unwrap_err(),
        Undiscovered::Unreached(HttpError::TimedOut.to_string())
    );
}

#[test]
fn a_document_listing_no_key_set_is_refused_before_its_signature_is_trusted() {
    let (config, _) = served();
    let page = Response {
        status: 200,
        headers: vec![("content-type".to_owned(), "text/html".to_owned())],
        body: b"<html>".to_vec(),
    };
    let (read, _) = read_listed(&config, page);
    assert_eq!(
        read.unwrap_err(),
        Undiscovered::Unkeyed(Unkeyed::Shape("it is no JSON"))
    );
}

#[test]
fn an_answer_is_held_to_the_config_s_fields_skew_and_the_longest_signature() {
    let (config, _) = served();
    let held = bounds(&config, at());
    assert_eq!(held.fields, config.federation.fields);
    assert_eq!(held.now, at().as_second());
    assert_eq!(held.skew, 300);
    assert_eq!(held.longest, 1200);
}

#[test]
fn every_reason_a_document_is_not_read_reads_as_a_sentence() {
    for (why, said) in [
        (
            Undiscovered::Unreached("refused".to_owned()),
            "the discovery document could not be fetched: refused",
        ),
        (
            Undiscovered::Status(404),
            "the discovery document was answered 404",
        ),
        (
            Undiscovered::Unanswered(Unanswered::Undigested),
            "the discovery document's answer is refused: the answer carries no digest of its body",
        ),
        (
            Undiscovered::Forged,
            "the discovery document's signature does not hold",
        ),
        (
            Undiscovered::Unkeyed(Unkeyed::Shape("it is no JSON")),
            "the key set cannot be read: it is no JSON",
        ),
    ] {
        assert_eq!(why.to_string(), said);
    }
}
