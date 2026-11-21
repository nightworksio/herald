//! What Discord says of a destination's credential and of each route's
//! channel, asked with nothing posted (ADR-0002).

use herald_kinds::{Audience, Probe};
use herald_ports::{Credential, Http, Method, SecretText};

use crate::discord::{DISCORD, Discord};
use crate::request::{bare, endpoint};

/// What Discord says of `discord`'s credential and, for a bot, of its
/// route's channel, asked through `http`.
pub(crate) async fn probe<H: Http>(discord: &Discord, http: &H) -> Vec<Probe> {
    let credential = &discord.credential;
    let asked = match credential {
        Credential::Bot(_) => SecretText::plain(format!("{}/users/@me", discord.api)),
        Credential::Webhook(url) => SecretText::secret(url.clone()),
    };
    let taken = Probe::credential(
        DISCORD,
        &http.send(bare(Method::Get, asked, credential.bot())).await,
    );
    let mut probes = vec![taken];
    if let (Some(Probe::Taken), Credential::Bot(_), Some(channel)) =
        (probes.first(), credential, &discord.route.channel)
    {
        let url = match endpoint(&discord.api, &["channels", channel]) {
            Ok(url) => url,
            Err(declined) => {
                probes.push(Probe::Route {
                    key: Some("channel"),
                    why: declined.to_string(),
                });
                return probes;
            }
        };
        let answer = http.send(bare(Method::Get, url, credential.bot())).await;
        let unseen = Probe::route(DISCORD, Some("channel"), &answer, &|status| {
            format!(
                "the bot cannot see the channel {channel}, as Discord answered {status}; add the bot to the server and give it View Channel and Send Messages there"
            )
        });
        if let Some(unseen) = unseen {
            probes.push(unseen);
            return probes;
        }
        let found = answer
            .ok()
            .and_then(|answer| serde_json::from_slice(&answer.body).ok())
            .ok_or_else(|| "Discord's answer did not say who may view the channel".to_owned());
        match crate::audience::seen(discord, http, (channel, found)).await {
            Audience::Private => {}
            Audience::Public(why) => probes.push(Probe::Public(why)),
            Audience::Unread(why) => probes.push(Probe::Said(format!(
                "herald withholds advisories sections from this channel: {why}"
            ))),
        }
    }
    probes
}
