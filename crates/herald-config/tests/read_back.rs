//! Each destination's `read_back`: the pages it reads back of its own posts,
//! five where unset, and refused outside one to twenty.

#[cfg(test)]
mod read_back {
    use herald_config::{Destination, load};
    use herald_core::{DestinationName, ReadBack};

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
        let Some(Destination::Discord(options)) =
            config.destinations.get(&DestinationName::new("discord"))
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
}
