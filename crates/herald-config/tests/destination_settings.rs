//! Each destination's own settings: the pages it reads back of its own
//! posts, the symbols it writes and how long it waits to send again.

#[cfg(test)]
mod destination_settings {
    use herald_config::{Destination, load};
    use herald_core::{DestinationName, ReadBack, Waits};
    use jiff::SignedDuration;

    fn config(destination: &str) -> Result<herald_config::Config, String> {
        let text = format!(
            r#"version = 1

[store]
kind = "markers"

[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
{destination}

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "1566200000000000100"
template = "release"

[templates.release]
title = "{{{{ event.project }}}} {{{{ event.version }}}}"
body = "{{{{ event.body }}}}"
"#
        );
        load("herald.toml", &text, &|name| {
            Err(format!("{name}: no such file"))
        })
        .map_err(|errors| errors.to_string())
    }

    fn read_back(config: &herald_config::Config) -> ReadBack {
        let Some(options) = config
            .destinations
            .get(&DestinationName::new("discord"))
            .and_then(|destination| destination.of::<herald_dest_discord::DestinationOptions>())
        else {
            panic!("no Discord destination");
        };
        options.read_back.unwrap_or_default()
    }

    #[test]
    fn a_destination_reads_back_the_pages_it_names_or_five() {
        assert_eq!(read_back(&config("").unwrap()), ReadBack::DEFAULT);
        assert_eq!(ReadBack::DEFAULT.pages(), 5);
        assert_eq!(
            read_back(&config("read_back = 20").unwrap()),
            ReadBack::new(20).unwrap()
        );
        assert_eq!(read_back(&config("read_back = 1").unwrap()).pages(), 1);
    }

    #[test]
    fn a_read_back_of_no_pages_or_past_twenty_is_refused() {
        for pages in [0, 21] {
            let refused = config(&format!("read_back = {pages}")).unwrap_err();
            assert!(
                refused.contains(&format!("read_back reads 1 to 20 pages, not {pages}")),
                "{refused}"
            );
        }
        assert_eq!(ReadBack::new(0), None);
        assert_eq!(ReadBack::new(21), None);
        assert_eq!(ReadBack::MOST.pages(), 20);
    }

    #[test]
    fn a_destination_s_symbols_reach_each_of_its_routes() {
        let set = config("symbols = { rule = \"=\", bullet = \"*\" }").unwrap();
        let symbols = set.routes[0].profile.symbols;
        assert_eq!((symbols.rule, symbols.bullet), ('=', Some('*')));
        assert_eq!(symbols.done, herald_core::Symbols::DEFAULT.done);
        let unset = config("").unwrap();
        assert_eq!(
            unset.routes[0].profile.symbols,
            herald_core::Symbols::DEFAULT
        );
        let refused = config("symbols = { rule = \"==\" }").unwrap_err();
        assert!(refused.contains("is not one character"), "{refused}");
    }

    fn waits(config: &herald_config::Config) -> Waits {
        config.destinations[&DestinationName::new("discord")].waits()
    }

    #[test]
    fn a_destination_waits_as_long_as_it_says_and_its_platform_s_own_elsewhere() {
        assert_eq!(waits(&config("").unwrap()), Waits::DEFAULT);
        assert_eq!(
            waits(&config("waits = { outage = \"30s\" }").unwrap()),
            Waits {
                outage: SignedDuration::from_secs(30),
                limited: Waits::DEFAULT.limited,
            }
        );
        let mastodon = |written: &str| {
            let options = toml::from_str::<herald_dest_fediverse::DestinationOptions>(&format!(
                "server = \"https://mastodon.example\"\ntoken = {{ secret = \"T\" }}\n{written}"
            ))
            .unwrap();
            Destination::new(options).waits()
        };
        assert_eq!(mastodon("").limited, SignedDuration::from_mins(5));
        assert_eq!(mastodon("").outage, Waits::DEFAULT.outage);
        assert_eq!(
            mastodon("waits = { limited = \"15m\" }").limited,
            SignedDuration::from_mins(15)
        );
    }

    #[test]
    fn a_wait_shorter_than_a_second_is_refused() {
        let refused = config("waits = { limited = \"500ms\" }").unwrap_err();
        assert!(
            refused.contains("waits.limited of 500ms would ask again at once; wait at least 1s"),
            "{refused}"
        );
        assert!(config("waits = { outage = \"1s\", limited = \"1s\" }").is_ok());
        let unknown = config("waits = { forever = \"1s\" }").unwrap_err();
        assert!(unknown.contains("forever"), "{unknown}");
    }
}
