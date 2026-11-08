//! `[destinations.*.interactions]` and a route's `subscribe`: commands each
//! destination answers and how, read and refused by key (ADR-0016).

#[cfg(test)]
mod interactions {
    use herald_config::{Destination, RouteOptions, load};
    use herald_core::{Command, Replies, RoleId};

    const APPLICATION: &str = "123456789012345678";
    const ROLE: &str = "223456789012345678";
    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn config(store: &str, discord: &str, route: &str) -> String {
        format!(
            r#"version = 1

[store]
kind = "{store}"

[destinations.discord]
kind = "discord"
{discord}

[destinations.slack]
kind = "slack"
token = {{ secret = "HERALD_SLACK_TOKEN" }}

[destinations.slack.interactions]
signing = {{ secret = "HERALD_SLACK_SIGNING" }}

[destinations.matrix]
kind = "matrix"
homeserver = "https://matrix.example"
token = {{ secret = "HERALD_MATRIX_TOKEN" }}

[destinations.matrix.interactions]
homeserver_token = {{ secret = "HERALD_MATRIX_HS" }}
appservice_token = {{ secret = "HERALD_MATRIX_AS" }}

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
template = "t"
{route}

[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
"#
        )
    }

    fn bot(interactions: &str) -> String {
        format!(
            "token = {{ secret = \"HERALD_DISCORD_TOKEN\" }}\n\n[destinations.discord.interactions]\napplication = \"{APPLICATION}\"\nkey = \"{KEY}\"\n{interactions}"
        )
    }

    fn errors(text: &str) -> String {
        load("herald.toml", text, &nothing).unwrap_err().to_string()
    }

    fn refused(text: &str, expected: &str) {
        let errors = errors(text);
        assert!(errors.contains(expected), "{errors}");
    }

    #[test]
    fn each_destination_reads_its_interactions_and_a_route_mentions_its_opt_in_role() {
        let text = config(
            "git-branch",
            &bot("replies = \"public\"\ncommands = [\"subscribe\", \"latest\"]"),
            &format!("channel = \"1566200000000000100\"\nsubscribe = {{ role = \"{ROLE}\" }}"),
        );
        let config = load("herald.toml", &text, &nothing).unwrap();
        let answering = config.destinations[&herald_core::DestinationName::new("discord")]
            .answering()
            .unwrap();
        assert_eq!(answering.commands, [Command::Subscribe, Command::Latest]);
        assert_eq!(answering.replies, Replies::Public);
        let route = &config.routes[0];
        assert_eq!(route.route.delivery.mentions.roles, [RoleId::new(ROLE)]);
        assert_eq!(route.options.opt_in_role().as_deref(), Some(ROLE));
        assert_eq!(route.options.place(), Some("1566200000000000100"));
        assert!(
            matches!(&route.options, RouteOptions::Discord(placed) if placed.subscribe.as_ref().is_some_and(|subscribe| subscribe.button))
        );
        for name in ["slack", "matrix"] {
            assert_eq!(
                config.destinations[&herald_core::DestinationName::new(name)]
                    .answering()
                    .map(|answering| answering.commands),
                Some(Command::ALL.to_vec())
            );
        }
        let secrets = config.secrets();
        for name in [
            "HERALD_SLACK_SIGNING",
            "HERALD_MATRIX_HS",
            "HERALD_MATRIX_AS",
        ] {
            assert!(secrets.contains(name), "{name}");
        }
    }

    #[test]
    fn an_opt_in_role_the_route_mentions_already_is_mentioned_once() {
        let text = config(
            "git-branch",
            &bot(""),
            &format!(
                "channel = \"1566200000000000100\"\nmentions = {{ roles = [\"{ROLE}\"] }}\nsubscribe = {{ role = \"{ROLE}\" }}"
            ),
        );
        let config = load("herald.toml", &text, &nothing).unwrap();
        assert_eq!(
            config.routes[0].route.delivery.mentions.roles,
            [RoleId::new(ROLE)]
        );
    }

    #[test]
    fn a_destination_answering_nothing_answers_no_command() {
        let text = config(
            "git-branch",
            "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
            "channel = \"1566200000000000100\"",
        );
        let config = load("herald.toml", &text, &nothing).unwrap();
        assert_eq!(
            config.destinations[&herald_core::DestinationName::new("discord")].answering(),
            None
        );
        assert!(matches!(
            config.destinations[&herald_core::DestinationName::new("discord")],
            Destination::Discord(_)
        ));
        assert_eq!(config.routes[0].options.opt_in_role(), None);
    }

    #[test]
    fn interactions_written_wrongly_are_refused_by_key() {
        refused(
            &config(
                "git-branch",
                &bot("guild = \"1\""),
                "channel = \"1566200000000000100\"",
            ),
            "destinations.discord.interactions: \"1\" is not a server id",
        );
        refused(
            &config(
                "git-branch",
                &format!(
                    "webhook = {{ secret = \"HERALD_DISCORD_WEBHOOK\" }}\n\n[destinations.discord.interactions]\napplication = \"{APPLICATION}\"\nvia = \"gateway\""
                ),
                "",
            ),
            "destinations.discord.interactions: the gateway is joined as a bot",
        );
        refused(
            &config(
                "git-branch",
                &bot("template = \"missing\""),
                "channel = \"1566200000000000100\"",
            ),
            "destinations.discord.interactions.template: no template is named \"missing\"; the templates are \"t\"",
        );
        refused(
            &config("markers", &bot(""), "channel = \"1566200000000000100\""),
            "destinations.discord.interactions: the markers store keeps nothing commands answer with",
        );
        let slack = config("git-branch", &bot(""), "channel = \"1566200000000000100\"").replace(
            "signing = { secret = \"HERALD_SLACK_SIGNING\" }",
            "command = \"herald\"\nsigning = { secret = \"S\" }",
        );
        refused(
            &slack,
            "destinations.slack.interactions: \"herald\" is not a slash command",
        );
        let matrix = config("git-branch", &bot(""), "channel = \"1566200000000000100\"")
            .replace("homeserver_token = { secret = \"HERALD_MATRIX_HS\" }\n", "");
        refused(
            &matrix,
            "destinations.matrix.interactions: an application service holds two tokens",
        );
    }

    #[test]
    fn a_webhook_destination_takes_the_interactions_discord_posts() {
        let text = config(
            "git-branch",
            &format!(
                "webhook = {{ secret = \"HERALD_DISCORD_WEBHOOK\" }}\n\n[destinations.discord.interactions]\napplication = \"{APPLICATION}\"\nkey = \"{KEY}\""
            ),
            "",
        );
        let config = load("herald.toml", &text, &nothing).unwrap();
        assert!(
            config.destinations[&herald_core::DestinationName::new("discord")]
                .answering()
                .is_some()
        );
    }

    #[test]
    fn a_fediverse_route_announces_in_no_channel_and_offers_no_role() {
        for options in [
            RouteOptions::Mastodon(herald_dest_fediverse::RouteOptions::default()),
            RouteOptions::ActivityPub(herald_dest_fediverse::ActorRouteOptions {
                actor: "herald".to_owned(),
                content_warning: None,
            }),
        ] {
            assert_eq!(options.place(), None);
            assert_eq!(options.opt_in_role(), None);
        }
    }

    #[test]
    fn a_markers_store_answers_no_command_not_even_subscribing() {
        let text = config(
            "markers",
            &bot("commands = [\"subscribe\"]"),
            "channel = \"1566200000000000100\"",
        )
        .replace(
            "[destinations.slack.interactions]\n",
            "[destinations.slack.interactions]\ncommands = [\"subscribe\"]\n",
        )
        .replace(
            "[destinations.matrix.interactions]\n",
            "[destinations.matrix.interactions]\ncommands = [\"unsubscribe\"]\n",
        );
        let errors = errors(&text);
        for destination in ["discord", "slack", "matrix"] {
            assert!(
                errors.contains(&format!(
                    "destinations.{destination}.interactions: the markers store keeps nothing commands answer with"
                )),
                "{errors}"
            );
        }
    }

    #[test]
    fn a_discord_subscribe_needs_a_role_id_a_bot_and_interactions_answering_it() {
        refused(
            &config(
                "git-branch",
                &bot(""),
                "channel = \"1566200000000000100\"\nsubscribe = { role = \"updates\" }",
            ),
            "routes[0].subscribe: \"updates\" is not a role id",
        );
        refused(
            &config(
                "git-branch",
                "webhook = { secret = \"HERALD_DISCORD_WEBHOOK\" }",
                &format!("subscribe = {{ role = \"{ROLE}\" }}"),
            ),
            "routes[0].subscribe: a member is given the opt-in role by a bot",
        );
        refused(
            &config(
                "git-branch",
                "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
                &format!("channel = \"1566200000000000100\"\nsubscribe = {{ role = \"{ROLE}\" }}"),
            ),
            "routes[0].subscribe: subscribing is answered through the destination's interactions; give [destinations.discord.interactions] answering subscribe",
        );
        refused(
            &config(
                "git-branch",
                &bot("commands = [\"latest\"]"),
                &format!("channel = \"1566200000000000100\"\nsubscribe = {{ role = \"{ROLE}\" }}"),
            ),
            "routes[0].subscribe: subscribing is answered through the destination's interactions",
        );
    }

    fn slack_route(route: &str) -> String {
        config("git-branch", &bot(""), "channel = \"1566200000000000100\"").replace(
            "[templates.t]",
            &format!(
                "[[routes]]\nid = \"slack\"\nsources = [\"gate\"]\ndestination = \"slack\"\ntemplate = \"t\"\nchannel = \"C0123456789\"\n{route}\n\n[templates.t]"
            ),
        )
    }

    #[test]
    fn a_slack_subscribe_needs_the_route_to_mention_subscribers_and_interactions_answering_it() {
        let text = slack_route("mentions = { subscribers = true }\nsubscribe = { button = false }");
        let config = load("herald.toml", &text, &nothing).unwrap();
        assert!(
            matches!(&config.routes[1].options, RouteOptions::Slack(placed) if placed.subscribe.as_ref().is_some_and(|subscribe| !subscribe.button))
        );
        refused(
            &slack_route("subscribe = {}"),
            "routes[1].subscribe: a member subscribes to be mentioned; add mentions = { subscribers = true } to the route",
        );
        refused(
            &slack_route("mentions = { subscribers = true }\nsubscribe = {}").replace(
                "signing = { secret = \"HERALD_SLACK_SIGNING\" }",
                "signing = { secret = \"HERALD_SLACK_SIGNING\" }\ncommands = [\"latest\"]",
            ),
            "routes[1].subscribe: subscribing is answered through the destination's interactions; give [destinations.slack.interactions] answering subscribe",
        );
    }

    #[test]
    fn sessions_connect_under_their_own_domains_and_those_their_destination_adds() {
        let http = config("git-branch", &bot(""), "channel = \"1566200000000000100\"");
        let config_of = |text: &str| load("herald.toml", text, &nothing).unwrap();
        assert!(config_of(&http).session_domains().is_empty());
        let gateway = config(
            "git-branch",
            &bot("via = \"gateway\"\ndomains = [\"proxy.example\"]"),
            "channel = \"1566200000000000100\"",
        );
        assert_eq!(
            config_of(&gateway)
                .session_domains()
                .into_iter()
                .collect::<Vec<_>>(),
            ["discord.gg", "proxy.example"]
        );
        let socket = http.replace(
            "signing = { secret = \"HERALD_SLACK_SIGNING\" }",
            "via = \"socket\"\napp = { secret = \"HERALD_SLACK_APP\" }",
        );
        assert_eq!(
            config_of(&socket)
                .session_domains()
                .into_iter()
                .collect::<Vec<_>>(),
            ["slack.com"]
        );
        refused(
            &config(
                "git-branch",
                &bot("via = \"gateway\"\ndomains = [\"https://proxy.example\"]"),
                "channel = \"1566200000000000100\"",
            ),
            "destinations.discord.interactions: \"https://proxy.example\" is not a domain",
        );
    }
}
