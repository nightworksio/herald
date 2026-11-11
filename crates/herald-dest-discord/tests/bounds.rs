//! Discord within its bounds: the buttons a post holds and the pins a forum
//! looks up.

#[cfg(test)]
mod bounds {
    use herald_core::{Delivery, Entry, Facts, PinLookups, Sent, Step};
    use herald_dest_discord::{Credential, Discord, RouteOptions};
    use herald_ports::Secret;
    use herald_testing::sample::{self, announcement};
    use herald_testing::{Fixture, Replay};
    use serde_json::Value;

    const CHANNEL: &str = "1566184281406177531";
    const FORUM: &str = "1566184281406177600";

    fn bot(channel: &str, forum: bool) -> Discord {
        Discord::new(
            Credential::Bot(Secret::new("HERALD_DISCORD_TOKEN", "test-token")),
            RouteOptions {
                channel: Some(channel.to_owned()),
                forum,
                subscribe: None,
            },
            None,
        )
    }

    fn delivery() -> Delivery {
        sample::delivery("1566190000000000001")
    }

    fn message(id: &str) -> Facts {
        Facts {
            message: Some(id.to_owned()),
            ..Facts::default()
        }
    }

    fn unpin(discord: &Discord, replay: &Replay, entry: &Entry) -> Sent {
        pollster::block_on(discord.send(replay, &Step::Unpin, &announcement(), &delivery(), entry))
            .unwrap()
    }

    fn gets(answers: &[(&str, u16, Value)]) -> Replay {
        let exchanges: Vec<Value> = answers
            .iter()
            .map(|(path, status, body)| {
                serde_json::json!({
                    "request": {
                        "method": "GET",
                        "url": format!("https://discord.com/api/v10{path}"),
                        "headers": {
                            "Authorization": "Bot «secret:HERALD_DISCORD_TOKEN»",
                            "User-Agent": "DiscordBot (https://github.com/nightworksio/herald, 0.0.0)",
                        },
                    },
                    "response": { "status": status, "body": body },
                })
            })
            .collect();
        Replay::of(
            Fixture::from_json(&serde_json::json!({ "exchanges": exchanges }).to_string()).unwrap(),
        )
    }

    #[test]
    fn a_forum_s_pins_are_looked_up_no_further_than_pin_lookups() {
        let entry =
            Entry::new(announcement().key).landed(Step::Post, message("1566200000000000010"));
        let me = ("/users/@me", 200, serde_json::json!({ "id": "9" }));
        let forum = format!("/channels/{FORUM}");
        let pinned = |id: &str| serde_json::json!({ "id": id, "parent_id": FORUM, "owner_id": "9", "flags": 2 });
        let replay = gets(&[
            me.clone(),
            (&forum, 200, serde_json::json!({ "guild_id": "1" })),
            (
                "/guilds/1/threads/active",
                200,
                serde_json::json!({ "threads": [pinned("11"), pinned("12")] }),
            ),
        ]);
        let one = bot(FORUM, true).looking_up(PinLookups::new(1).unwrap());
        assert_eq!(
            unpin(&one, &replay, &entry),
            Sent::Refused("the route's earlier pins are among 2 pins, more than the 1 it looks up; raise pin_lookups, up to 100, or unpin them by hand".to_owned())
        );
        replay.finish().unwrap();
        let unplaced = gets(&[me, (&forum, 200, serde_json::json!({}))]);
        assert_eq!(
            unpin(&bot(FORUM, true), &unplaced, &entry),
            Sent::Refused(format!(
                "Discord did not say which server the forum {FORUM} is in"
            ))
        );
        unplaced.finish().unwrap();
        assert_eq!(
            unpin(&bot("", false), &gets(&[]), &entry),
            Sent::Refused("the route names no Discord channel; set its channel".to_owned())
        );
    }

    #[test]
    fn buttons_are_drawn_five_to_a_row_each_to_an_address_discord_takes() {
        let link = |name: &str, url: String| herald_core::Button {
            name: name.to_owned(),
            label: None,
            url,
        };
        let longest = format!("https://example.org/{}", "a".repeat(512 - 20));
        let mut many = announcement();
        many.buttons = (0..7)
            .map(|index| {
                link(
                    &format!("link_{index}"),
                    format!("https://example.org/{index}"),
                )
            })
            .chain([
                link("longest", longest.clone()),
                link("longer", format!("{longest}a")),
            ])
            .collect();
        let request = bot(CHANNEL, false)
            .request(&Step::Post, &many, &delivery(), &Entry::new(many.key))
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let rows: Vec<Vec<&str>> = body["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                assert_eq!(row["type"], 1);
                row["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|button| button["label"].as_str().unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(
            rows,
            [
                vec!["link_0", "link_1", "link_2", "link_3", "link_4"],
                vec!["link_5", "link_6", "longest"],
            ]
        );
    }
}
