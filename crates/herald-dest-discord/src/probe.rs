//! What Discord says of a destination's credential and of each route's
//! channel, asked with nothing posted (ADR-0002).

use herald_core::Delivery;
use herald_kinds::{Audience, Probe, Recorded};
use herald_ports::{Credential, Http, Method, SecretText};
use serde::Deserialize;

use crate::audience::{Channel, server, within};
use crate::discord::{DISCORD, Discord};
use crate::permissions::{Placed, lacking};
use crate::request::{bare, endpoint};

/// The bot, as Discord answers for its token.
#[derive(Deserialize)]
struct Bot {
    id: String,
}

/// What Discord says of `discord`'s credential and, for a bot, of its
/// route's channel and of the permissions a route delivering as
/// `delivery`, its announcements `recorded` so, needs there, asked through
/// `http`.
pub(crate) async fn probe<H: Http>(
    discord: &Discord,
    http: &H,
    needs: (&Delivery, Recorded),
) -> Vec<Probe> {
    let credential = &discord.credential;
    let asked = match credential {
        Credential::Bot(_) => SecretText::plain(format!("{}/users/@me", discord.api)),
        Credential::Webhook(url) => SecretText::secret(url.clone()),
    };
    let answer = http.send(bare(Method::Get, asked, credential.bot())).await;
    let taken = Probe::credential(DISCORD, &answer);
    let mut probes = vec![taken];
    if let (Some(Probe::Taken), Credential::Bot(_), Some(channel)) =
        (probes.first(), credential, &discord.route.channel)
    {
        let bot = answer
            .ok()
            .and_then(|answer| serde_json::from_slice::<Bot>(&answer.body).ok());
        probes.extend(channelled(discord, http, (channel, bot), needs).await);
    }
    probes
}

/// What Discord says, asked through `http`, of the route's channel
/// `channel`, of what the bot `bot` may do there of what `needs` asks, and
/// of who may view it.
async fn channelled<H: Http>(
    discord: &Discord,
    http: &H,
    (channel, bot): (&str, Option<Bot>),
    needs: (&Delivery, Recorded),
) -> Vec<Probe> {
    let url = match endpoint(&discord.api, &["channels", channel]) {
        Ok(url) => url,
        Err(declined) => {
            return vec![Probe::Route {
                key: Some("channel"),
                why: declined.to_string(),
            }];
        }
    };
    let answer = http
        .send(bare(Method::Get, url, discord.credential.bot()))
        .await;
    let unseen = Probe::route(DISCORD, Some("channel"), &answer, &|status| {
        format!(
            "the bot cannot see the channel {channel}, as Discord answered {status}; add the bot to the server and give it View Channel and Send Messages there"
        )
    });
    if let Some(unseen) = unseen {
        return vec![unseen];
    }
    let withheld = |why: String| {
        Probe::Said(format!(
            "herald withholds advisories sections from this channel: {why}"
        ))
    };
    let Some(found) = answer
        .ok()
        .and_then(|answer| serde_json::from_slice::<Channel>(&answer.body).ok())
    else {
        return vec![withheld(
            "Discord's answer did not say who may view the channel".to_owned(),
        )];
    };
    let unchecked = |why: &str| {
        Probe::Said(format!(
            "herald does not check the bot's permissions in the channel: {why}"
        ))
    };
    let placed = server(discord, http, &found).await;
    let mut probes = match (&placed, &bot) {
        (Ok((guild, server)), Some(bot)) => {
            let placed = Placed {
                guild,
                server,
                channel: &found,
            };
            lacking(discord, http, (&bot.id, placed), needs).await
        }
        (Ok(_), None) => vec![unchecked("Discord did not say who the bot is")],
        (Err(why), _) => vec![unchecked(why)],
    };
    match within(discord, http, (channel, &found), placed).await {
        Audience::Private => {}
        Audience::Public(why) => probes.push(Probe::Public(why)),
        Audience::Unread(why) => probes.push(withheld(why)),
    }
    probes
}
