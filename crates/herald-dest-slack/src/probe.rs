//! What Slack says of a bot token and of the scopes each route needs, asked
//! with nothing posted (ADR-0002).

use herald_core::Delivery;
use herald_kinds::Probe;
use herald_ports::{Http, Secret, Text};
use serde_json::{Map, Value};

use crate::read::{json, quoted, text};
use crate::request::{get, posted_json};
use crate::slack::{Credential, Slack};
use crate::web_api::{
    AUTH_TEST, BOTS_INFO, CHAT_POST_MESSAGE, CONVERSATIONS_HISTORY, PINS_ADD, PINS_LIST,
};

/// The platform, as a probe names it.
const SLACK: &str = "Slack";
/// What Slack answers an empty post to a live incoming webhook: its
/// payload is refused, and nothing reaches the channel.
const LIVE: u16 = 400;
/// What Slack answers a post to an incoming webhook that no longer posts:
/// its token is refused, it was removed, or its channel was archived.
const DEAD: [u16; 3] = [403, 404, 410];
/// The header Slack lists a token's scopes in.
const SCOPES: &str = "x-oauth-scopes";

/// The scopes a route delivering as `delivery` needs, each a choice of
/// scopes any one of which serves.
fn needed(delivery: &Delivery) -> Vec<&'static [&'static str]> {
    let each = |scopes: &'static [&'static str]| scopes.chunks(1);
    let mut needed: Vec<&[&str]> = each(CHAT_POST_MESSAGE.scopes)
        .chain(each(BOTS_INFO.scopes))
        .collect();
    needed.push(CONVERSATIONS_HISTORY.scopes);
    if delivery.pin.is_some() {
        needed.extend(each(PINS_ADD.scopes).chain(each(PINS_LIST.scopes)));
    }
    needed
}

/// What a bot token granted `granted`, a comma-separated list, lacks of
/// what `delivery` needs.
fn lacking(granted: &str, delivery: &Delivery) -> Vec<String> {
    let granted: Vec<&str> = granted.split(',').collect();
    needed(delivery)
        .into_iter()
        .filter(|choice| !choice.iter().any(|scope| granted.contains(scope)))
        .map(|choice| choice.join(" or "))
        .collect()
}

/// What Slack answers an empty post to the incoming webhook `url`, which
/// it refuses before anything reaches the channel.
async fn webhook<H: Http>(slack: &Slack, http: &H, url: &Secret) -> Probe {
    let empty = posted_json(slack, Text::secret(url.clone()), &Value::Object(Map::new()));
    match http.send(empty).await {
        Ok(answer) if answer.status == LIVE => Probe::Taken,
        Ok(answer) if DEAD.contains(&answer.status) => Probe::Credential(format!(
            "Slack refuses its webhook, answering {}: {}",
            answer.status,
            quoted(&String::from_utf8_lossy(&answer.body))
        )),
        other => Probe::credential(SLACK, &other),
    }
}

/// What Slack says of `slack`'s bot token and of the scopes `delivery`
/// needs, asked through `http`.
pub(crate) async fn probe<H: Http>(slack: &Slack, http: &H, delivery: &Delivery) -> Vec<Probe> {
    if let Credential::Webhook(url) = &slack.credential {
        return vec![webhook(slack, http, url).await];
    }
    let answer = match http.send(get(slack, AUTH_TEST, "")).await {
        Ok(answer) if answer.succeeded() => answer,
        other => return vec![Probe::credential(SLACK, &other)],
    };
    let tested = json(&answer);
    if tested.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = quoted(text(&tested, "error").unwrap_or("an unnamed error"));
        return vec![Probe::Credential(format!(
            "Slack refuses its credential: {error}"
        ))];
    }
    if tested.get("is_enterprise_install").and_then(Value::as_bool) == Some(true)
        && slack.team.is_none()
    {
        return vec![Probe::Credential("the token is installed across an Enterprise Grid organization, so each call names a workspace; set team to the workspace's id".to_owned())];
    }
    let mut probes = vec![Probe::Taken];
    let lacks = lacking(answer.header(SCOPES).unwrap_or_default(), delivery);
    if !lacks.is_empty() {
        probes.push(Probe::Route {
            key: None,
            why: format!("the bot token lacks the scopes {}; add them to the app's bot token scopes and install it again", lacks.join(", ")),
        });
    }
    probes
}
