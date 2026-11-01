use herald_core::{Announcement, Entry, Facts, Marker, Sent};
use herald_ports::{Http, HttpError};
use serde_json::Value;

use crate::discord::{Credential, Discord};
use crate::markers::{author, marks};
use crate::read::trouble;
use crate::request::{Refusal, get, posted, unpin};

/// The most pins Discord lists in one page.
const PINS: u8 = 50;
/// What Discord answers to unpinning a message no longer there.
const GONE: u16 = 404;

async fn read<H: Http>(
    discord: &Discord,
    http: &H,
    path: &str,
) -> Result<Result<Value, Sent>, HttpError> {
    let response = http.send(get(discord, path)).await?;
    let body = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    Ok(trouble(&response, &body).map_or(Ok(body), Err))
}

async fn stale<H: Http>(
    discord: &Discord,
    http: &H,
    channel: &str,
    route: &Marker,
) -> Result<Result<Vec<String>, Sent>, HttpError> {
    let user = match read(discord, http, "/users/@me").await? {
        Ok(user) => user,
        Err(sent) => return Ok(Err(sent)),
    };
    let Some(bot) = user.get("id").and_then(Value::as_str) else {
        return Ok(Err(Sent::Refused(
            "Discord did not say which user the bot is".to_owned(),
        )));
    };
    let path = format!("/channels/{channel}/messages/pins?limit={PINS}");
    let pins = match read(discord, http, &path).await? {
        Ok(pins) => pins,
        Err(sent) => return Ok(Err(sent)),
    };
    Ok(Ok(pins
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|pin| pin.get("message"))
        .filter(|message| author(message) == Some(bot))
        .filter(|message| {
            marks(message)
                .iter()
                .any(|(marked, _)| marked == route.as_str())
        })
        .filter_map(|message| message.get("id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()))
}

/// Unpins the route's earlier pins: the one the store remembers, or else
/// each of the bot's own pins carrying the route's marker. The
/// announcement's own pin stays.
pub(crate) async fn unpinned<H: Http>(
    discord: &Discord,
    http: &H,
    announcement: &Announcement,
    entry: &Entry,
) -> Result<Sent, HttpError> {
    if let Credential::Webhook(_) = discord.credential {
        return Ok(Sent::Refused(Refusal::WebhookCannot("unpin").to_string()));
    }
    let Some(channel) = discord.route.channel.as_deref() else {
        return Ok(Sent::Refused(Refusal::NoChannel.to_string()));
    };
    let current = match posted(entry) {
        Ok(current) => current.to_owned(),
        Err(refusal) => return Ok(Sent::Refused(refusal.to_string())),
    };
    let earlier = match &announcement.replaces {
        Some(earlier) => vec![earlier.clone()],
        None => match stale(discord, http, channel, &announcement.marks.route).await? {
            Ok(earlier) => earlier,
            Err(sent) => return Ok(sent),
        },
    };
    for message in earlier.iter().filter(|message| **message != current) {
        let response = http.send(unpin(discord, channel, message)).await?;
        if response.status == GONE {
            continue;
        }
        let body = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
        if let Some(sent) = trouble(&response, &body) {
            return Ok(sent);
        }
    }
    Ok(Sent::Landed(Facts::default()))
}
