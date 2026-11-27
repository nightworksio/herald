use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use super::*;
use crate::{Method, Request, Secret, SecretError, SecretText, Secrets};

fn shortest(characters: usize) -> NonZeroUsize {
    NonZeroUsize::new(characters).unwrap_or(NonZeroUsize::MIN)
}

fn installation_token(seed: &str) -> String {
    format!("ghs_{}", seed.repeat(36_usize.div_ceil(seed.len())))
        .chars()
        .take(40)
        .collect()
}

#[test]
fn a_kept_secret_and_each_of_its_lines_are_scrubbed() {
    let scrubber = Scrubber::new(shortest(8));
    let first = installation_token("aB3");
    let second = installation_token("Zq9");
    scrubber.keep(&Secret::new("HERALD_KEY", format!("{first}\n{second}")));
    assert_eq!(
        scrubber.scrubbed(&format!("a: {first} b: {second}.")),
        "a: «secret:HERALD_KEY» b: «secret:HERALD_KEY»."
    );
    assert_eq!(
        scrubber.scrubbed(&format!("both: {first}\n{second}")),
        "both: «secret:HERALD_KEY»"
    );
}

#[test]
fn a_value_shorter_than_the_shortest_is_left_and_a_lower_shortest_scrubs_it() {
    let pin = ["4", "7", "1", "9", "2", "6"].concat();
    let line = format!("the pin {pin} is shown");
    let default = Scrubber::new(shortest(8));
    default.keep(&Secret::new("PIN", pin.as_str()));
    assert_eq!(default.scrubbed(&line), line);
    let lowered = Scrubber::new(shortest(6));
    lowered.keep(&Secret::new("PIN", pin.as_str()));
    assert_eq!(lowered.scrubbed(&line), "the pin «secret:PIN» is shown");
    let higher = Scrubber::new(shortest(7));
    higher.keep(&Secret::new("PIN", pin.as_str()));
    assert_eq!(higher.scrubbed(&line), line);
}

#[test]
fn a_blank_value_is_not_kept_and_a_value_is_kept_once() {
    let scrubber = Scrubber::new(shortest(1));
    scrubber.keep(&Secret::new("BLANK", "  "));
    assert_eq!(scrubber.scrubbed("a  b"), "a  b");
    let token = installation_token("x1");
    scrubber.keep(&Secret::new("FIRST", token.as_str()));
    scrubber.keep(&Secret::new("SECOND", token.as_str()));
    assert_eq!(scrubber.scrubbed(&token), "«secret:FIRST»");
}

#[test]
fn the_longest_value_is_matched_first() {
    let scrubber = Scrubber::new(shortest(8));
    let short = installation_token("mN4");
    let long = format!("{short}-and-more");
    scrubber.keep(&Secret::new("SHORT", short.as_str()));
    scrubber.keep(&Secret::new("LONG", long.as_str()));
    assert_eq!(
        scrubber.scrubbed(&format!("{long} {short}")),
        "«secret:LONG» «secret:SHORT»"
    );
}

#[test]
fn a_minted_token_a_request_carries_is_scrubbed_by_every_clone() {
    let scrubber = Scrubber::new(shortest(8));
    let token = installation_token("Pk7");
    let hook = installation_token("w2W");
    let request = Request::new(
        Method::Post,
        SecretText::plain("https://hooks.example/")
            .then_secret(Secret::new("the hook", hook.as_str())),
    )
    .bearer(Secret::new("github_apps.release", token.as_str()))
    .header("Accept", SecretText::plain("application/json"));
    let clone = scrubber.clone();
    clone.sent(&request);
    assert_eq!(
        scrubber.scrubbed(&format!("refused {token} at {hook}; application/json")),
        "refused «secret:github_apps.release» at «secret:the hook»; application/json"
    );
}

#[test]
fn a_value_sent_again_is_held_once_and_the_one_sent_longest_ago_is_forgotten_first() {
    let scrubber = Scrubber::new(shortest(8));
    let sending = |value: &str| {
        Request::new(Method::Get, SecretText::plain("https://api.example/"))
            .bearer(Secret::new("minted", value))
    };
    let oldest = installation_token("Oo1");
    scrubber.sent(&sending(&oldest));
    scrubber.sent(&sending(&oldest));
    let kept = installation_token("Kk2");
    scrubber.keep(&Secret::new("KEPT", kept.as_str()));
    scrubber.sent(&sending(&kept));
    scrubber.sent(&Request::new(
        Method::Get,
        SecretText::secret(Secret::new("blank", " ")),
    ));
    let values: Vec<String> = (1..Scrubber::SENT)
        .map(|index| format!("{}{index:05}", installation_token("Nn3")))
        .collect();
    for value in &values {
        scrubber.sent(&sending(value));
    }
    assert_eq!(scrubber.scrubbed(&oldest), "«secret:minted»");
    assert_eq!(scrubber.scrubbed(&kept), "«secret:KEPT»");
    let newest = installation_token("Nw4");
    scrubber.sent(&sending(&newest));
    assert_eq!(scrubber.scrubbed(&oldest), oldest);
    assert_eq!(scrubber.scrubbed(&values[0]), "«secret:minted»");
    assert_eq!(scrubber.scrubbed(&newest), "«secret:minted»");
    assert_eq!(scrubber.scrubbed(&kept), "«secret:KEPT»");
}

struct Named(BTreeMap<&'static str, String>);

impl Secrets for Named {
    fn secret(&self, name: &str) -> impl Future<Output = Result<Secret, SecretError>> + Send {
        std::future::ready(
            self.0
                .get(name)
                .map(|value| Secret::new(name, value.as_str()))
                .ok_or_else(|| SecretError::Missing {
                    name: name.to_owned(),
                }),
        )
    }
}

#[test]
fn each_secret_read_through_kept_secrets_is_scrubbed() {
    let token = installation_token("Rd5");
    let secrets = Named(BTreeMap::from([("HERALD_TOKEN", token.clone())]));
    let scrubber = Scrubber::new(shortest(8));
    let kept = Kept::new(&secrets, &scrubber);
    assert_eq!(
        pollster::block_on(kept.secret("HERALD_TOKEN")).map(|secret| secret.expose().to_owned()),
        Ok(token.clone())
    );
    assert_eq!(
        pollster::block_on(kept.secret("UNSET")),
        Err(SecretError::Missing {
            name: "UNSET".to_owned()
        })
    );
    assert_eq!(scrubber.scrubbed(&token), "«secret:HERALD_TOKEN»");
    assert!(format!("{scrubber:?}").contains("kept: [«secret:HERALD_TOKEN»]"));
}
