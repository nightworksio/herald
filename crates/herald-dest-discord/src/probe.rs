//! What Discord says of a destination's credential and of each route's
//! channel, asked with nothing posted (ADR-0002).

use herald_kinds::Probe;
use herald_ports::{Credential, Http, Method, Text, unreserved};

use crate::discord::{DISCORD, Discord};
use crate::request::bare;

/// What Discord says of `discord`'s credential and, for a bot, of its
/// route's channel, asked through `http`.
pub(crate) async fn probe<H: Http>(discord: &Discord, http: &H) -> Vec<Probe> {
    let credential = &discord.credential;
    let asked = match credential {
        Credential::Bot(_) => Text::plain(format!("{}/users/@me", discord.api)),
        Credential::Webhook(url) => Text::secret(url.clone()),
    };
    let taken = Probe::credential(
        DISCORD,
        &http.send(bare(Method::Get, asked, credential.bot())).await,
    );
    let mut probes = vec![taken];
    if let (Some(Probe::Taken), Credential::Bot(_), Some(channel)) =
        (probes.first(), credential, &discord.route.channel)
    {
        let url = format!("{}/channels/{}", discord.api, unreserved(channel));
        let answer = http
            .send(bare(Method::Get, Text::plain(url), credential.bot()))
            .await;
        probes.extend(Probe::route(DISCORD, Some("channel"), &answer, &|status| {
            format!("the bot cannot see the channel {channel}, as Discord answered {status}; add the bot to the server and give it View Channel and Send Messages there")
        }));
    }
    probes
}
