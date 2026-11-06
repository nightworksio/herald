//! Whatever Discord, Slack or a homeserver sends, each destination answers
//! what it asks or why it cannot read it, and every command it reads names
//! where it was given and by whom, with Slack's answers going to Slack alone.

#![no_main]

use herald_core::{Asked, DestinationName};
use herald_dest_discord::{
    Effect, Gateway, Interacting as Discord, Interactions as DiscordInteractions,
};
use herald_dest_matrix::{Interacting as Matrix, Interactions as MatrixInteractions};
use herald_dest_slack::{Interacting as Slack, Interactions as SlackInteractions, Received};
use herald_ports::Secret;
use libfuzzer_sys::fuzz_target;

fn placed(asked: &Asked) {
    assert!(!asked.place.id.is_empty());
    assert!(
        asked
            .place
            .parent
            .as_ref()
            .is_none_or(|parent| !parent.is_empty())
    );
    assert!(!asked.user.is_empty());
}

fuzz_target!(|bytes: &[u8]| {
    let Some((&chosen, body)) = bytes.split_first() else {
        return;
    };
    let destination = DestinationName::new("fuzzed");
    match chosen % 4 {
        0 => {
            let interactions: DiscordInteractions =
                serde_json::from_str(r#"{"application": "123456789012345678", "via": "gateway"}"#)
                    .unwrap();
            let discord = Discord::new(interactions, None, None);
            if let Ok(herald_dest_discord::Received::Asked { asked, .. }) =
                discord.received(&destination, body)
            {
                placed(&asked);
            }
            let mut gateway = Gateway::new(Secret::new("T", "token"));
            for frame in String::from_utf8_lossy(body).split('\n') {
                for effect in gateway.received(&discord, &destination, frame) {
                    if let Effect::Received(received) = effect
                        && let herald_dest_discord::Received::Asked { asked, .. } = *received
                    {
                        placed(&asked);
                    }
                }
            }
        }
        1 => {
            let interactions: SlackInteractions =
                serde_json::from_str(r#"{"signing": {"secret": "S"}}"#).unwrap();
            let slack = Slack::new(interactions, None);
            if let Ok(Received::Asked { asked, replying }) = slack.received(&destination, body) {
                placed(&asked);
                assert!(
                    replying
                        .url
                        .expose()
                        .starts_with("https://hooks.slack.com/")
                );
            }
            let enveloped = slack.envelope(&destination, &String::from_utf8_lossy(body));
            if let Some(Received::Asked { asked, replying }) = enveloped.received {
                placed(&asked);
                assert!(
                    replying
                        .url
                        .expose()
                        .starts_with("https://hooks.slack.com/")
                );
            }
        }
        _ => {
            let interactions: MatrixInteractions =
                serde_json::from_str(r#"{"via": "sync"}"#).unwrap();
            let matrix = Matrix::new(
                "https://matrix.example",
                interactions,
                Secret::new("T", "token"),
            );
            for (asked, _) in matrix.transaction(&destination, body).unwrap_or_default() {
                placed(&asked);
            }
            if let Ok(synced) = matrix.synced(&destination, body) {
                for (asked, _) in synced.asked {
                    placed(&asked);
                }
            }
        }
    }
});
