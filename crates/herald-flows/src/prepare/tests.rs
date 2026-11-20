use herald_core::{Phrase, ReadBack, Wording};
use herald_dest_discord::{Credential, Discord};
use herald_dest_fediverse::{ActivityPub, Mastodon};
use herald_dest_matrix::Matrix;
use herald_dest_slack::Slack;
use serde_json::json;

use super::*;

#[test]
fn a_secret_read_first_is_taken_again_and_one_not_read_is_missing() {
    let read = ReadSecrets([("TOKEN".to_owned(), Secret::new("TOKEN", "t"))].into());
    assert_eq!(
        pollster::block_on(read.secret("TOKEN")),
        Ok(Secret::new("TOKEN", "t"))
    );
    let missing = pollster::block_on(read.secret("OTHER")).unwrap_err();
    assert_eq!(
        PrepareError::from(missing.clone()).to_string(),
        "no secret is named OTHER; set it where this platform keeps secrets"
    );
    let named = UnreadSecret {
        error: missing,
        keys: vec!["a.token".to_owned(), "b.token".to_owned()],
    };
    assert_eq!(
        PrepareError::Unread(vec![named.clone(), named]).to_string(),
        ["a.token, b.token: no secret is named OTHER; set it where this platform keeps secrets"; 2]
            .join("\n")
    );
}

fn reached(destination: &Destination, options: &Placed) -> Option<Reached> {
    Resolved {
        destination,
        base: "",
        secret: Secret::new("HERALD_TOKEN", "token"),
        read: Vec::new(),
        marker_key: Secret::new("HERALD_MARKERS_KEY", "key"),
        derived: false,
    }
    .reached(options)
}

#[test]
fn each_destination_reads_back_and_looks_as_its_config_says() {
    let pages = ReadBack::new(9).unwrap();
    let token = json!({ "secret": "HERALD_TOKEN" });
    let secret = || Secret::new("HERALD_TOKEN", "token");
    let discord = Destination::new(
        serde_json::from_value::<herald_dest_discord::DestinationOptions>(json!({
            "token": token, "read_back": 9,
            "colours": { "release": "#112233" }, "archive_after": "1d",
            "interactions": { "application": "1566184281406177000", "wording": { "release_link": "Uitgave" } }
        }))
        .unwrap(),
    );
    let slack = Destination::new(
        serde_json::from_value::<herald_dest_slack::DestinationOptions>(
            json!({ "token": token, "read_back": 9, "domain": "slack-gov.com", "team": "T0123456789" }),
        )
        .unwrap(),
    );
    let matrix = Destination::new(
        serde_json::from_value::<herald_dest_matrix::DestinationOptions>(json!({
            "homeserver": "https://matrix.example", "token": token, "read_back": 9
        }))
        .unwrap(),
    );
    let mastodon = Destination::new(
        serde_json::from_value::<herald_dest_fediverse::DestinationOptions>(json!({
            "server": "https://mastodon.example", "token": token, "read_back": 9
        }))
        .unwrap(),
    );
    let room = herald_dest_matrix::RouteOptions {
        room: "!abcdefg:matrix.example".to_owned(),
    };
    assert_eq!(
        reached(
            &discord,
            &Placed::new(herald_dest_discord::RouteOptions::default())
        ),
        Some(Reached::new(
            Discord::new(
                Credential::Bot(secret()),
                herald_dest_discord::RouteOptions::default(),
                None
            )
            .reading_back(pages)
            .looking(herald_dest_discord::Look {
                release: herald_dest_discord::Colour(0x0011_2233),
                archive_after: herald_dest_discord::ArchiveAfter::Day,
                ..herald_dest_discord::Look::DEFAULT
            })
            .worded(Wording::of(Some(
                &[(Phrase::ReleaseLink, "Uitgave".to_owned())].into()
            )))
        ))
    );
    assert_eq!(
        reached(
            &slack,
            &Placed::new(herald_dest_slack::RouteOptions::default())
        ),
        Some(Reached::new(
            Slack::new(
                herald_dest_slack::Credential::Bot(secret()),
                herald_dest_slack::RouteOptions::default(),
                None
            )
            .on(herald_dest_slack::Domain::Gov)
            .in_team(Some("T0123456789"))
            .reading_back(pages)
        ))
    );
    assert_eq!(
        reached(&matrix, &Placed::new(room.clone())),
        Some(Reached::new(
            Matrix::new("https://matrix.example", secret(), room).reading_back(pages)
        ))
    );
    assert_eq!(
        reached(
            &mastodon,
            &Placed::new(herald_dest_fediverse::RouteOptions::default())
        ),
        Some(Reached::new(
            Mastodon::new(
                "https://mastodon.example",
                secret(),
                herald_dest_fediverse::RouteOptions::default()
            )
            .reading_back(pages)
        ))
    );
}

#[test]
fn each_destination_waits_as_its_config_says_and_its_platform_elsewhere() {
    let token = json!({ "secret": "HERALD_TOKEN" });
    let secret = || Secret::new("HERALD_TOKEN", "token");
    let outage = jiff::SignedDuration::from_secs(40);
    let waits = json!({ "outage": "40s" });
    let matrix = Destination::new(
        serde_json::from_value::<herald_dest_matrix::DestinationOptions>(
            json!({ "homeserver": "https://matrix.example", "token": token, "waits": waits }),
        )
        .unwrap(),
    );
    let mastodon = Destination::new(
        serde_json::from_value::<herald_dest_fediverse::DestinationOptions>(
            json!({ "server": "https://mastodon.example", "token": token, "waits": waits }),
        )
        .unwrap(),
    );
    let actor = Destination::new(
        serde_json::from_value::<herald_dest_fediverse::ActorOptions>(
            json!({ "domain": "herald.example", "key": token, "waits": waits }),
        )
        .unwrap(),
    );
    let room = herald_dest_matrix::RouteOptions {
        room: "!abcdefg:matrix.example".to_owned(),
    };
    let toot = herald_dest_fediverse::RouteOptions::default();
    let named: herald_dest_fediverse::ActorRouteOptions =
        serde_json::from_value(json!({ "actor": "herald" })).unwrap();
    assert_eq!(
        reached(&matrix, &Placed::new(room.clone())),
        Some(Reached::new(
            Matrix::new("https://matrix.example", secret(), room).waiting(herald_core::Waits {
                outage,
                ..herald_core::Waits::DEFAULT
            })
        ))
    );
    assert_eq!(
        reached(&mastodon, &Placed::new(toot.clone())),
        Some(Reached::new(
            Mastodon::new("https://mastodon.example", secret(), toot).waiting(herald_core::Waits {
                outage,
                ..Mastodon::WAITS
            })
        ))
    );
    assert_eq!(
        reached(&actor, &Placed::new(named.clone())),
        Some(Reached::new(
            ActivityPub::new("herald.example", "", secret(), named).waiting(herald_core::Waits {
                outage,
                ..herald_core::Waits::DEFAULT
            })
        ))
    );
}

#[test]
fn a_slack_on_its_government_cloud_takes_commands_there() {
    let slack = Destination::new(
        serde_json::from_value::<herald_dest_slack::DestinationOptions>(json!({
            "token": { "secret": "HERALD_TOKEN" },
            "domain": "slack-gov.com",
            "interactions": { "via": "socket", "app": { "secret": "HERALD_SLACK_APP" } },
        }))
        .unwrap(),
    );
    let secrets = herald_testing::FakeSecrets::default().with("HERALD_SLACK_APP", "xapp");
    let interacting =
        pollster::block_on(slack.interacts(&Secret::new("HERALD_TOKEN", "token"), &secrets))
            .unwrap();
    let interacting = interacting.unwrap();
    let interacting = interacting.of::<herald_dest_slack::Interacting>().unwrap();
    assert_eq!(
        (interacting.api(), interacting.hooks()),
        ("https://slack-gov.com/api", "https://hooks.slack-gov.com")
    );
}
