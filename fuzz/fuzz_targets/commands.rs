//! Whatever Discord, Slack, Teams or a homeserver sends, each destination
//! answers what it asks or why it cannot read it, and every command it reads
//! names where it was given and by whom, with Slack's answers going to Slack
//! alone; a Teams bot takes an activity only of Teams, naming the service URL
//! its token vouches for, and a command in it, or a press of its subscribe
//! button, only as the member its sender names, answered through that
//! service URL under the message the member gave or the card they pressed.

#![no_main]

use herald_core::{Asked, DestinationName};
use herald_dest_discord::{
    Effect, Gateway, Interacting as Discord, Interactions as DiscordInteractions, SESSION_DOMAIN,
};
use herald_dest_matrix::{Interacting as Matrix, Interactions as MatrixInteractions};
use herald_dest_slack::{Interacting as Slack, Interactions as SlackInteractions, Received};
use herald_dest_teams::{CHANNEL, Cloud, Interacting as Teams, Reply, SUBSCRIBE};
use herald_kinds::{Interacts as _, Post, Read, Vouched};
use herald_ports::Secret;
use libfuzzer_sys::fuzz_target;

const TEAMS_APP: &str = "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
const TEAMS_SERVICE: &str = "https://smba.trafficmanager.net/emea/";

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
    match chosen % 5 {
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
            let mut gateway = Gateway::new(Secret::new("T", "token"), &[SESSION_DOMAIN]);
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
        3 => {
            let teams = Teams::new(TEAMS_APP, Secret::new("fuzz", ""), Cloud::Public.hosts()).unwrap();
            let vouched = Vouched {
                claims: vec![("serviceUrl", TEAMS_SERVICE.to_owned())],
            };
            let read = teams.read(
                &destination,
                Post {
                    method: "POST",
                    path: &[],
                    body,
                    vouched: Some(&vouched),
                },
            );
            if let Read::Delivered { then, .. } = read {
                let activity: serde_json::Value = serde_json::from_slice(body).unwrap();
                assert_eq!(activity["serviceUrl"], TEAMS_SERVICE);
                assert_eq!(activity["channelId"], CHANNEL);
                if let Read::Asked { asked, .. } = *then {
                    for (asked, replying) in &asked {
                        placed(asked);
                        let pressed = activity["type"] == "invoke";
                        assert!(pressed || activity["type"] == "message");
                        assert_eq!(pressed, asked.toggles);
                        assert_eq!(
                            activity["from"]["aadObjectId"]
                                .as_str()
                                .map(str::to_ascii_lowercase),
                            Some(asked.user.clone())
                        );
                        let reply = replying.of::<Reply>().unwrap();
                        assert_eq!(format!("{}/", reply.service), TEAMS_SERVICE);
                        assert_eq!(activity["conversation"]["id"], reply.conversation.as_str());
                        let answered = if pressed { "replyToId" } else { "id" };
                        assert_eq!(activity[answered], reply.activity.as_str());
                        if pressed {
                            assert_eq!(activity["value"]["action"]["verb"], SUBSCRIBE);
                            assert_eq!(
                                activity["value"]["action"]["data"]["project"].as_str(),
                                asked.project.as_deref()
                            );
                        }
                    }
                }
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
                for (asked, _, _) in synced.asked {
                    placed(&asked);
                }
            }
        }
    }
});
