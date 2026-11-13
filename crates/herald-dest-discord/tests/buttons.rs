//! Discord's buttons, drawn within what a post holds.

#[cfg(test)]
mod buttons {
    use herald_core::{Entry, Step};
    use herald_dest_discord::{Credential, Discord, RouteOptions};
    use herald_ports::Secret;
    use herald_testing::sample::{self, announcement};

    fn bot() -> Discord {
        Discord::new(
            Credential::Bot(Secret::new("HERALD_DISCORD_TOKEN", "test-token")),
            RouteOptions {
                channel: Some("1566184281406177531".to_owned()),
                forum: false,
                subscribe: None,
            },
            None,
        )
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
        let request = bot()
            .request(
                &Step::Post,
                &many,
                &sample::delivery("1566190000000000001"),
                &Entry::new(many.key),
            )
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
