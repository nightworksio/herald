//! Each destination's options in `herald.toml`: what loads, and what each
//! destination cannot do refused by key.

#[cfg(test)]
mod destinations {
    use herald_config::{Destination, RouteOptions, load};

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn errors(text: &str) -> String {
        load("herald.toml", text, &nothing).unwrap_err().to_string()
    }

    /// The name of the secret the config's first destination reaches its
    /// platform with.
    fn credential(config: &herald_config::Config) -> Option<&str> {
        config
            .destinations
            .values()
            .next()
            .and_then(Destination::credential)
            .map(|reference| reference.secret.as_str())
    }

    const MASTODON: &str = r#"version = 1

[store]
kind = "sqlite"

[destinations.social]
kind = "mastodon"
server = "https://mastodon.example"
token = { secret = "HERALD_MASTODON_TOKEN" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "tooted"
sources = ["gate"]
destination = "social"
template = "t"
visibility = "unlisted"
language = "en"
content_warning = "Release notes"
limit = 1000
thread = { name = "{{ event.version }}" }

[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#;

    #[test]
    fn a_mastodon_destination_and_its_route_options_load() {
        let config = load("herald.toml", MASTODON, &nothing).unwrap();
        let Some(Destination::Mastodon(social)) = config.destinations.values().next() else {
            panic!("the destination is Mastodon's");
        };
        assert_eq!(social.server, "https://mastodon.example");
        assert_eq!(credential(&config), Some("HERALD_MASTODON_TOKEN"));
        assert_eq!(
            config.destinations.values().next().map(Destination::kind),
            Some("mastodon")
        );
        let RouteOptions::Mastodon(placed) = &config.routes[0].options else {
            panic!("the route's options are Mastodon's");
        };
        assert_eq!(
            placed.visibility,
            herald_dest_fediverse::Visibility::Unlisted
        );
        assert_eq!(placed.language.as_deref(), Some("en"));
        assert_eq!(placed.content_warning.as_deref(), Some("Release notes"));
        assert_eq!(
            config.routes[0].profile,
            herald_dest_fediverse::profile(placed)
        );
        assert!(!config.routes[0].options.threads_each_post());
        assert_eq!(
            config.secrets().into_iter().collect::<Vec<_>>(),
            ["HERALD_MASTODON_TOKEN"]
        );
    }

    #[test]
    fn a_mastodon_route_is_refused_what_mastodon_cannot_do() {
        let asking = MASTODON.replace(
            "thread = { name = \"{{ event.version }}\" }",
            "thread = { name = \"v\", archive_after = \"14d\" }\ncrosspost = true\npin = \"markers\"\nmentions = { roles = [\"1\"] }",
        );
        let error = errors(&asking);
        for key in ["crosspost", "pin", "mentions.roles", "thread.archive_after"] {
            assert!(
                error.contains(&format!(
                    "routes[0].{key}: the destination \"social\" cannot do this as it is reached; remove {key}, as Mastodon has no way to do it"
                )),
                "{key}: {error}"
            );
        }
        assert!(!error.contains("routes[0].thread:"), "{error}");
        let misspoken = MASTODON.replace("language = \"en\"", "language = \"English\"");
        assert!(
            errors(&misspoken).contains("routes[0]: \"English\" is not an ISO 639 language code"),
            "{}",
            errors(&misspoken)
        );
        let cramped = MASTODON.replace("limit = 1000", "limit = 100");
        assert!(
            errors(&cramped).contains("routes[0]: a status needs at least 141 characters"),
            "{}",
            errors(&cramped)
        );
        let foreign = MASTODON.replace("visibility = \"unlisted\"", "channel = \"1\"");
        assert!(
            errors(&foreign).contains("routes[0].channel: unknown field `channel`"),
            "{}",
            errors(&foreign)
        );
        let serverless = MASTODON.replace("server = \"https://mastodon.example\"\n", "");
        assert!(
            errors(&serverless).contains("destinations.social: missing field `server`"),
            "{}",
            errors(&serverless)
        );
    }

    const MATRIX: &str = r#"version = 1

[store]
kind = "sqlite"

[destinations.room]
kind = "matrix"
homeserver = "https://matrix.example"
token = { secret = "HERALD_MATRIX_TOKEN" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "roomed"
sources = ["gate"]
destination = "room"
template = "t"
room = "!releases:matrix.example"
pin = "markers"
mentions = { subscribers = true }
thread = { name = "{{ event.version }}" }

[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#;

    #[test]
    fn a_matrix_destination_and_its_route_options_load() {
        let config = load("herald.toml", MATRIX, &nothing).unwrap();
        let Some(Destination::Matrix(room)) = config.destinations.values().next() else {
            panic!("the destination is Matrix's");
        };
        assert_eq!(room.homeserver, "https://matrix.example");
        assert_eq!(credential(&config), Some("HERALD_MATRIX_TOKEN"));
        assert_eq!(
            config.destinations.values().next().map(Destination::kind),
            Some("matrix")
        );
        let RouteOptions::Matrix(placed) = &config.routes[0].options else {
            panic!("the route's options are Matrix's");
        };
        assert_eq!(placed.room, "!releases:matrix.example");
        assert_eq!(config.routes[0].profile, herald_dest_matrix::profile());
        assert_eq!(
            config.secrets().into_iter().collect::<Vec<_>>(),
            ["HERALD_MATRIX_TOKEN"]
        );
    }

    #[test]
    fn a_matrix_route_is_refused_what_matrix_cannot_do() {
        let asking = MATRIX.replace(
            "thread = { name = \"{{ event.version }}\" }",
            "thread = { name = \"v\", archive_after = \"14d\" }\ncrosspost = true",
        );
        let error = errors(&asking);
        for key in ["crosspost", "thread.archive_after"] {
            assert!(
                error.contains(&format!(
                    "routes[0].{key}: the destination \"room\" cannot do this as it is reached; remove {key}, as Matrix has no way to do it"
                )),
                "{key}: {error}"
            );
        }
        let aliased = MATRIX.replace("!releases:matrix.example", "#releases:matrix.example");
        assert!(
            errors(&aliased).contains("routes[0]: \"#releases:matrix.example\" is not a room id"),
            "{}",
            errors(&aliased)
        );
        for unsound in ["!:matrix.example", "!releases:", "!releases"] {
            assert!(
                errors(&MATRIX.replace("!releases:matrix.example", unsound))
                    .contains("is not a room id"),
                "{unsound}"
            );
        }
        let roomless = MATRIX.replace("room = \"!releases:matrix.example\"\n", "");
        assert!(
            errors(&roomless).contains("routes[0]: missing field `room`"),
            "{}",
            errors(&roomless)
        );
        let homeless = MATRIX.replace("homeserver = \"https://matrix.example\"\n", "");
        assert!(
            errors(&homeless).contains("destinations.room: missing field `homeserver`"),
            "{}",
            errors(&homeless)
        );
    }

    const SLACK: &str = r#"version = 1

[store]
kind = "sqlite"

[destinations.team]
kind = "slack"
token = { secret = "HERALD_SLACK_TOKEN" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "slacked"
sources = ["gate"]
destination = "team"
template = "t"
channel = "C0123456789"
pin = "markers"
mentions = { subscribers = true }
thread = { name = "{{ event.version }}" }

[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#;

    #[test]
    fn a_slack_destination_and_its_route_options_load_as_a_bot_or_a_webhook() {
        let config = load("herald.toml", SLACK, &nothing).unwrap();
        let Some(Destination::Slack(team)) = config.destinations.values().next() else {
            panic!("the destination is Slack's");
        };
        assert!(team.token.is_some());
        assert_eq!(credential(&config), Some("HERALD_SLACK_TOKEN"));
        assert_eq!(
            config.destinations.values().next().map(Destination::kind),
            Some("slack")
        );
        let RouteOptions::Slack(placed) = &config.routes[0].options else {
            panic!("the route's options are Slack's");
        };
        assert_eq!(placed.channel.as_deref(), Some("C0123456789"));
        assert_eq!(config.routes[0].profile, herald_dest_slack::profile(true));
        assert_eq!(
            config.secrets().into_iter().collect::<Vec<_>>(),
            ["HERALD_SLACK_TOKEN"]
        );
        let hooked = SLACK
            .replace("token = { secret = \"HERALD_SLACK_TOKEN\" }", "webhook = { secret = \"HERALD_SLACK_WEBHOOK\" }")
            .replace("channel = \"C0123456789\"\npin = \"markers\"\nmentions = { subscribers = true }\nthread = { name = \"{{ event.version }}\" }\n", "");
        let config = load("herald.toml", &hooked, &nothing).unwrap();
        assert_eq!(config.routes[0].profile, herald_dest_slack::profile(false));
        assert_eq!(credential(&config), Some("HERALD_SLACK_WEBHOOK"));
        assert_eq!(
            config.secrets().into_iter().collect::<Vec<_>>(),
            ["HERALD_SLACK_WEBHOOK"]
        );
    }

    #[test]
    fn a_slack_route_is_refused_what_slack_cannot_do_as_it_is_reached() {
        let asking = SLACK.replace(
            "thread = { name = \"{{ event.version }}\" }",
            "thread = { name = \"v\", archive_after = \"14d\" }\ncrosspost = true\nmentions.roles = [\"1\"]",
        ).replace("mentions = { subscribers = true }\n", "");
        let error = errors(&asking);
        for key in ["crosspost", "thread.archive_after", "mentions.roles"] {
            assert!(
                error.contains(&format!(
                    "routes[0].{key}: the destination \"team\" cannot do this as it is reached; remove {key}, as Slack has no way to do it"
                )),
                "{key}: {error}"
            );
        }
        let hooked = SLACK.replace(
            "token = { secret = \"HERALD_SLACK_TOKEN\" }",
            "webhook = { secret = \"HERALD_SLACK_WEBHOOK\" }",
        );
        let error = errors(&hooked);
        for key in ["pin", "thread"] {
            assert!(
                error.contains(&format!(
                    "routes[0].{key}: the destination \"team\" cannot do this as it is reached; remove {key}, or reach it with a bot token"
                )),
                "{key}: {error}"
            );
        }
        let hooked_crosspost = hooked.replace("pin = \"markers\"", "crosspost = true");
        assert!(
            errors(&hooked_crosspost).contains("routes[0].crosspost: the destination \"team\" cannot do this as it is reached; remove crosspost, as Slack has no way to do it"),
            "{}",
            errors(&hooked_crosspost)
        );
        let named = SLACK.replace("C0123456789", "#releases");
        assert!(
            errors(&named).contains("routes[0]: \"#releases\" is not a channel id"),
            "{}",
            errors(&named)
        );
        let unchannelled = SLACK.replace("channel = \"C0123456789\"\n", "");
        assert!(
            errors(&unchannelled).contains("routes[0]: a Slack bot posts to a channel"),
            "{}",
            errors(&unchannelled)
        );
        let both = SLACK.replace(
            "token = { secret = \"HERALD_SLACK_TOKEN\" }",
            "token = { secret = \"HERALD_SLACK_TOKEN\" }\nwebhook = { secret = \"HERALD_SLACK_WEBHOOK\" }",
        );
        assert!(
            errors(&both).contains(
                "destinations.team: a Slack destination takes a token or a webhook, not both"
            ),
            "{}",
            errors(&both)
        );
        let neither = SLACK.replace("token = { secret = \"HERALD_SLACK_TOKEN\" }\n", "");
        assert!(
            errors(&neither)
                .contains("destinations.team: a Slack destination needs a token or a webhook"),
            "{}",
            errors(&neither)
        );
    }

    const ACTIVITYPUB: &str = r#"version = 1

[store]
kind = "git-branch"

[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "followed"
sources = ["gate"]
destination = "fedi"
template = "t"
actor = "php-mutation-gate"
content_warning = "Release notes"

[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#;

    #[test]
    fn an_activitypub_destination_and_its_actor_load() {
        let config = load("herald.toml", ACTIVITYPUB, &nothing).unwrap();
        let Some(Destination::ActivityPub(fedi)) = config.destinations.values().next() else {
            panic!("the destination is herald's own actor's");
        };
        assert_eq!(fedi.domain, "herald.example");
        assert_eq!(credential(&config), Some("HERALD_ACTIVITYPUB_KEY"));
        assert_eq!(
            config.destinations.values().next().map(Destination::kind),
            Some("activitypub")
        );
        let RouteOptions::ActivityPub(placed) = &config.routes[0].options else {
            panic!("the route's options are the actor's");
        };
        assert_eq!(placed.actor, "php-mutation-gate");
        assert_eq!(placed.content_warning.as_deref(), Some("Release notes"));
        assert_eq!(
            config.routes[0].profile,
            herald_dest_fediverse::actor_profile()
        );
        assert!(
            config
                .secrets()
                .into_iter()
                .any(|secret| secret == "HERALD_ACTIVITYPUB_KEY")
        );
    }

    #[test]
    fn an_activitypub_route_is_refused_a_bad_domain_or_actor_what_it_cannot_do_and_a_store_without_followers()
     {
        let schemed = ACTIVITYPUB.replace(
            "domain = \"herald.example\"",
            "domain = \"https://herald.example\"",
        );
        assert!(
            errors(&schemed)
                .contains("destinations.fedi: \"https://herald.example\" is not a domain"),
            "{}",
            errors(&schemed)
        );
        let named = ACTIVITYPUB.replace("actor = \"php-mutation-gate\"", "actor = \"PHP Gate\"");
        assert!(
            errors(&named).contains("routes[0]: \"PHP Gate\" is not an actor's name"),
            "{}",
            errors(&named)
        );
        let pinning = ACTIVITYPUB.replace(
            "content_warning = \"Release notes\"",
            "pin = \"markers\"\ncrosspost = true",
        );
        let error = errors(&pinning);
        for key in ["pin", "crosspost"] {
            assert!(
                error.contains(&format!(
                    "routes[0].{key}: the destination \"fedi\" cannot do this as it is reached; remove {key}, as herald's ActivityPub actor has no way to do it"
                )),
                "{key}: {error}"
            );
        }
        let actorless = ACTIVITYPUB.replace("actor = \"php-mutation-gate\"\n", "");
        assert!(
            errors(&actorless).contains("routes[0]: missing field `actor`"),
            "{}",
            errors(&actorless)
        );
        let marked = ACTIVITYPUB.replace("kind = \"git-branch\"", "kind = \"markers\"");
        assert!(
            errors(&marked).contains("routes[0]: the markers store keeps no subscriptions, so this route has no followers or subscribers to reach; give a store that keeps them, such as \"git-branch\""),
            "{}",
            errors(&marked)
        );
        let mentioning = MATRIX.replace("kind = \"sqlite\"", "kind = \"markers\"");
        assert!(
            errors(&mentioning).contains("routes[0]: the markers store keeps no subscriptions"),
            "{}",
            errors(&mentioning)
        );
        let quiet = MATRIX
            .replace("kind = \"sqlite\"", "kind = \"markers\"")
            .replace("mentions = { subscribers = true }\n", "");
        assert!(load("herald.toml", &quiet, &nothing).is_ok());
    }
}
