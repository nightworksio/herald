//! `herald.toml` read whole, and every mistake in it named by line, column,
//! key and fix.

#[cfg(test)]
mod load {
    use herald_config::{Destination, RouteOptions, Source, Store, load, schema};
    use herald_core::{Kind, Pinning, Prereleases};
    use herald_sources_forge::Forge;

    const EXAMPLE: &str = r#"#:schema https://raw.githubusercontent.com/nightworksio/herald/main/herald.schema.json
version = 1

[store]
kind = "git-branch"

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.php-mutation-gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "php-mutation-gate-releases"
sources = ["php-mutation-gate"]
kinds = ["release"]
destination = "discord"
channel = "1566184281406177531"
template = "package-release"
crosspost = true
thread = { name = "{{ event.project }} {{ event.version }}", archive_after = "14d" }
mentions = { roles = ["<the php-mutation-gate role's id>"] }

[templates.package-release]
title = "📦 {{ event.project }} {{ event.version }}{% if event.prerelease %} · pre-release{% endif %}"
body = "{{ event.body }}"
"#;

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn errors(text: &str) -> String {
        load("herald.toml", text, &nothing).unwrap_err().to_string()
    }

    #[test]
    fn the_documented_example_loads() {
        let config = load("herald.toml", EXAMPLE, &nothing).unwrap();
        assert_eq!(
            config.store,
            Store::GitBranch(herald_store_git_branch::StoreOptions::default())
        );
        assert_eq!(config.marker_key, None);
        assert!(matches!(
            config.destinations.values().next(),
            Some(Destination::Discord(options)) if options.token.is_some()
        ));
        assert_eq!(
            config.destinations.values().next().map(Destination::kind),
            Some("discord")
        );
        assert!(matches!(
            config.sources.values().next(),
            Some(Source::Forge(Forge::GitHub, options)) if options.repository == "nightworksio/php-mutation-gate"
        ));
        let route = &config.routes[0];
        assert_eq!(route.route.id.as_str(), "php-mutation-gate-releases");
        assert_eq!(route.route.filter.kinds, [Kind::Release]);
        assert_eq!(route.route.filter.prereleases, Prereleases::Include);
        assert!(route.route.delivery.crosspost);
        assert_eq!(route.route.delivery.pin, None);
        let thread = route.route.delivery.thread.as_ref().unwrap();
        assert_eq!(thread.archive_after.unwrap().as_secs(), 1_209_600);
        assert_eq!(thread.lock_after, None);
        assert_eq!(route.route.delivery.mentions.roles.len(), 1);
        assert!(matches!(
            &route.options,
            RouteOptions::Discord(options) if options.channel.as_deref() == Some("1566184281406177531")
        ));
        assert!(config.apps.is_empty());
        assert_eq!(config.schedule.poll, None);
    }

    #[test]
    fn every_section_reads_into_its_type() {
        let text = format!(
            "{EXAMPLE}\n[markers]\nkey = {{ secret = \"HERALD_MARKER_SECRET\" }}\n\n[schedule]\npoll = \"15m\"\nhousekeep = \"1h\"\n\n[templates.from-file]\ntitle = \"{{{{ event.title }}}}\"\nfile = \"templates/body.md.j2\"\n"
        );
        let read = |file: &str| {
            assert_eq!(file, "templates/body.md.j2");
            Ok("{{ event.body }}".to_owned())
        };
        let config = load("herald.toml", &text, &read).unwrap();
        assert_eq!(config.marker_key.unwrap().secret, "HERALD_MARKER_SECRET");
        assert_eq!(config.schedule.poll.unwrap().as_secs(), 900);
        assert_eq!(config.schedule.housekeep.unwrap().as_secs(), 3_600);
    }

    #[test]
    fn a_name_that_resolves_to_nothing_names_what_there_is() {
        let text = EXAMPLE.replace(
            "destination = \"discord\"",
            "destination = \"announcements\"",
        );
        assert_eq!(
            errors(&text),
            "herald.toml:19:15 routes[0].destination: no destination is named \"announcements\"; the destinations are \"discord\""
        );
        let text = EXAMPLE
            .replace(
                "sources = [\"php-mutation-gate\"]",
                "sources = [\"herald\"]",
            )
            .replace("template = \"package-release\"", "template = \"other\"");
        assert_eq!(
            errors(&text),
            "herald.toml:17:12 routes[0].sources: no source or app is named \"herald\"; the sources and apps are \"php-mutation-gate\"\nherald.toml:21:12 routes[0].template: no template is named \"other\"; the templates are \"package-release\""
        );
    }

    #[test]
    fn a_plain_string_where_a_secret_belongs_is_refused_without_repeating_it() {
        let text = EXAMPLE.replace(
            "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
            "token = \"a-token-written-plainly\"",
        );
        let error = errors(&text);
        assert!(error.starts_with("herald.toml:9:9 destinations.discord.token: a secret is written as { secret = \"NAME\" }"), "{error}");
        assert!(!error.contains("written-plainly"), "{error}");
    }

    #[test]
    fn every_mistake_is_reported_in_one_pass_in_file_order() {
        let text = "version = 2\nunknown = 1\n[store]\nkind = \"nowhere\"\n[destinations.d]\nkind = \"discord\"\n[sources.s]\nkind = \"bitbucket\"\n";
        assert_eq!(
            errors(text),
            [
                "herald.toml:1:11 version: this herald reads version 1 of the config; write version = 1",
                "herald.toml:2:1 unknown: herald.toml has no key \"unknown\"; the keys are \"version\", \"store\", \"markers\", \"destinations\", \"sources\", \"routes\", \"templates\", \"apps\", \"schedule\", \"server\"",
                "herald.toml:4:8 store.kind: there is no kind \"nowhere\"; the kinds are \"sqlite\", \"postgres\", \"d1\", \"dynamodb\", \"deno-kv\", \"markers\", \"git-branch\", \"actions-cache\", \"repo-variables\"",
                "herald.toml:5:1 destinations.d: a Discord destination needs a token or a webhook; give one",
                "herald.toml:8:8 sources.s.kind: there is no kind \"bitbucket\"; the kinds are \"github\", \"gitlab\", \"forgejo\", \"packagist\", \"crates-io\", \"npm\", \"docker-hub\", \"feed\"",
            ]
            .join("\n")
        );
    }

    #[test]
    fn a_missing_version_or_store_and_broken_toml_are_named() {
        let error = errors("routes = 1\n[destinations]\nd = 3\n[x\n");
        assert!(
            error.contains("herald.toml:1:1 version: give the config's version: version = 1"),
            "{error}"
        );
        assert!(
            error.contains("herald.toml:1:1 store: choose a store"),
            "{error}"
        );
        assert!(
            error.contains("routes: routes are an array of tables"),
            "{error}"
        );
        assert!(
            error.contains("destinations.d: destinations.d is a table"),
            "{error}"
        );
        assert!(error.contains("herald.toml:4:"), "{error}");
    }

    #[test]
    fn every_secret_the_config_names_is_listed_once() {
        let text = EXAMPLE
            .replace(
                "kind = \"git-branch\"",
                "kind = \"git-branch\"\ntoken = { secret = \"STATE_TOKEN\" }",
            )
            .replace(
                "repository = \"nightworksio/php-mutation-gate\"",
                "repository = \"nightworksio/php-mutation-gate\"\ntoken = { secret = \"HERALD_DISCORD_TOKEN\" }",
            );
        let config = load("herald.toml", &text, &|_| Err(String::new())).unwrap();
        let secrets: Vec<&str> = config.secrets().into_iter().collect();
        assert_eq!(secrets, ["HERALD_DISCORD_TOKEN", "STATE_TOKEN"]);
        let variables = EXAMPLE.replace(
            "kind = \"git-branch\"",
            "kind = \"repo-variables\"\ntoken = { secret = \"VARIABLES_TOKEN\" }",
        );
        let keyed =
            format!("{variables}\n[markers]\nkey = {{ secret = \"HERALD_MARKER_SECRET\" }}\n");
        let config = load("herald.toml", &keyed, &|_| Err(String::new())).unwrap();
        assert!(config.secrets().contains("VARIABLES_TOKEN"));
        assert!(config.secrets().contains("HERALD_MARKER_SECRET"));
        let marked = EXAMPLE.replace("kind = \"git-branch\"", "kind = \"markers\"");
        let config = load("herald.toml", &marked, &|_| Err(String::new())).unwrap();
        assert_eq!(config.secrets().len(), 1);
    }

    #[test]
    fn every_store_kind_loads_as_itself() {
        for kind in Store::KINDS {
            let options = match kind {
                "repo-variables" => "\ntoken = { secret = \"HERALD_STATE_TOKEN\" }",
                "postgres" => "\nurl = { secret = \"HERALD_POSTGRES_URL\" }",
                _ => "",
            };
            let text = EXAMPLE.replace(
                "kind = \"git-branch\"",
                &format!("kind = \"{kind}\"{options}"),
            );
            let config = load("herald.toml", &text, &|_| Err(String::new())).unwrap();
            assert_eq!(config.store.kind(), kind);
        }
    }

    #[test]
    fn every_source_kind_loads_as_itself() {
        for kind in Source::KINDS {
            let forge = ["github", "gitlab", "forgejo"].contains(&kind);
            let mut text = EXAMPLE.replace("kind = \"github\"", &format!("kind = \"{kind}\""));
            if kind == "feed" {
                text = text
                    .replace(
                        "repository = \"nightworksio/php-mutation-gate\"",
                        "url = \"https://blog.example/feed.xml\"",
                    )
                    .replace("kinds = [\"release\"]", "kinds = [\"feed-entry\"]");
            } else if !forge {
                text = text
                    .replace("repository = ", "package = ")
                    .replace("kinds = [\"release\"]", "kinds = [\"package-version\"]");
            }
            let config = load("herald.toml", &text, &|_| Err(String::new())).unwrap();
            let source = config.sources.values().next().unwrap();
            assert_eq!(source.kind(), kind);
            assert!(config.secrets().contains("HERALD_DISCORD_TOKEN"));
            let made = match kind {
                "feed" => Kind::FeedEntry,
                _ if forge => Kind::Release,
                _ => Kind::PackageVersion,
            };
            assert_eq!(source.kinds(), [made]);
        }
    }

    #[test]
    fn a_route_s_versions_pattern_is_read_and_refused_where_its_kinds_carry_none() {
        let text = EXAMPLE.replace(
            "kinds = [\"release\"]",
            "kinds = [\"release\"]\nversions = \"v*.*.*\"",
        );
        let config = load("herald.toml", &text, &nothing).unwrap();
        let versions = config.routes[0].route.filter.versions.as_ref().unwrap();
        assert_eq!(versions.as_str(), "v*.*.*");
        let feeds = text.replace(
            "kinds = [\"release\"]",
            "kinds = [\"release\", \"feed-entry\"]",
        );
        let refused = errors(&feeds);
        assert!(
            refused.starts_with("herald.toml:19:12 routes[0].versions: feed entries and app messages carry no version"),
            "{refused}"
        );
    }

    #[test]
    fn only_npm_takes_a_token_among_the_registries() {
        let registry = |kind: &str| {
            EXAMPLE
                .replace("kind = \"github\"", &format!("kind = \"{kind}\""))
                .replace(
                    "repository = \"nightworksio/php-mutation-gate\"",
                    "package = \"nightworksio/php-mutation-gate\"\ntoken = { secret = \"REGISTRY_TOKEN\" }",
                )
                .replace("kinds = [\"release\"]", "kinds = [\"package-version\"]")
        };
        let config = load("herald.toml", &registry("npm"), &|_| Err(String::new())).unwrap();
        assert!(config.secrets().contains("REGISTRY_TOKEN"));
        let refused = errors(&registry("crates-io"));
        assert!(
            refused.starts_with("herald.toml:11:1 sources.php-mutation-gate.token: the registry \"crates-io\" takes no token; remove token\n"),
            "{refused}"
        );
    }

    #[test]
    fn an_option_its_destination_lacks_is_an_error_not_a_no_op() {
        let text = EXAMPLE
            .replace("token = { secret = \"HERALD_DISCORD_TOKEN\" }", "webhook = { secret = \"HERALD_DISCORD_WEBHOOK\" }")
            .replace("crosspost = true", "crosspost = true\npin = \"markers\"\nmentions = { roles = [\"1\"], subscribers = true }\nchannel_id = \"2\"")
            .replace("mentions = { roles = [\"<the php-mutation-gate role's id>\"] }\n", "");
        let error = errors(&text);
        assert!(
            error.contains(
                "routes[0].channel_id: unknown field `channel_id`, expected one of `channel`, `forum`, `subscribe`"
            ),
            "{error}"
        );
        let text = text.replace("channel_id = \"2\"\n", "");
        let error = errors(&text);
        for key in [
            "crosspost",
            "pin",
            "mentions.subscribers",
            "thread",
            "thread.archive_after",
        ] {
            assert!(
                error.contains(&format!(
                    "routes[0].{key}: the destination \"discord\" cannot do this as it is reached"
                )),
                "{key}: {error}"
            );
        }
        assert!(!error.contains("mentions.roles"), "{error}");
        for (key, remedy) in [
            ("crosspost", "or reach it with a bot token"),
            ("pin", "or reach it with a bot token"),
            ("thread", "or reach it with a bot token"),
            ("mentions.subscribers", "as Discord has no way to do it"),
        ] {
            assert!(
                error.contains(&format!("routes[0].{key}: the destination \"discord\" cannot do this as it is reached; remove {key}, {remedy}")),
                "{key}: {error}"
            );
        }
        let forum = text
            .replace("crosspost = true\npin = \"markers\"\n", "forum = true\n")
            .replace("mentions = { roles = [\"1\"], subscribers = true }\n", "")
            .replace("archive_after = \"14d\"", "lock_after = \"14d\"");
        let error = errors(&forum);
        assert!(!error.contains("routes[0].thread:"), "{error}");
        assert!(
            error.contains("routes[0].thread.lock_after: the destination \"discord\" cannot do this as it is reached"),
            "{error}"
        );
        let unchannelled = EXAMPLE.replace("channel = \"1566184281406177531\"\n", "");
        assert!(errors(&unchannelled).contains("routes[0]: a Discord bot posts to a channel"));
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

    #[test]
    fn a_template_that_does_not_compile_or_render_or_fit_is_found_at_load() {
        let broken = EXAMPLE.replace("body = \"{{ event.body }}\"", "body = \"{{ event.body \"");
        let error = errors(&broken);
        assert!(
            error.starts_with(
                "herald.toml:28:8 templates.package-release.body: the template does not compile:"
            ),
            "{error}"
        );
        let undefined = EXAMPLE.replace(
            "body = \"{{ event.body }}\"",
            "body = \"{{ event.release_notes }}\"",
        );
        let error = errors(&undefined);
        assert!(error.starts_with("herald.toml:28:8 templates.package-release.body: it does not render a sample Release event:"), "{error}");
        let long = EXAMPLE.replace("title = \"📦", &format!("title = \"{}", "x".repeat(300)));
        let error = errors(&long);
        assert!(
            error.contains("templates.package-release.title: it renders 3"),
            "{error}"
        );
        assert!(
            error.contains("over the destination's 256; shorten it"),
            "{error}"
        );
        let title = |length: usize| {
            EXAMPLE.replace(
                "title = \"📦 {{ event.project }} {{ event.version }}{% if event.prerelease %} · pre-release{% endif %}\"",
                &format!("title = \"{}\"", "x".repeat(length)),
            )
        };
        assert!(load("herald.toml", &title(256), &nothing).is_ok());
        assert!(errors(&title(257)).contains("it renders 257 characters"));
        let thread = EXAMPLE.replace(
            "{{ event.project }} {{ event.version }}\", archive_after",
            "{{ event.nope }}\", archive_after",
        );
        assert!(errors(&thread).contains("routes[0].thread.name: it does not render"));
    }

    #[test]
    fn a_template_needs_one_body_and_a_readable_file() {
        let both = EXAMPLE.replace(
            "body = \"{{ event.body }}\"",
            "body = \"x\"\nfile = \"b.md\"",
        );
        assert!(
            errors(&both).contains("templates.package-release: give its body or a file, not both")
        );
        let neither = EXAMPLE.replace("body = \"{{ event.body }}\"\n", "");
        assert!(errors(&neither).contains("templates.package-release: give its body"));
        let missing = EXAMPLE.replace("body = \"{{ event.body }}\"", "file = \"b.md\"");
        assert!(
            errors(&missing)
                .contains("templates.package-release.file: b.md could not be read: no file")
        );
    }

    #[test]
    fn two_routes_with_one_id_are_refused() {
        let twice = format!(
            "{EXAMPLE}\n{}",
            &EXAMPLE[EXAMPLE.find("[[routes]]").unwrap()..EXAMPLE.find("[templates").unwrap()]
        );
        assert!(
            errors(&twice)
                .contains("routes[1].id: another route has the id \"php-mutation-gate-releases\"")
        );
    }

    #[test]
    fn a_kind_that_is_not_a_string_or_absent_and_a_section_that_is_not_a_table_say_so() {
        let error = errors(
            "version = 1\nstore = 3\n[sources.s]\nkind = 1\n[destinations.d]\ntoken = { secret = \"T\" }\n[markers]\nkey = 1\n",
        );
        assert!(
            error.contains("store: store is a table; write [store]"),
            "{error}"
        );
        assert!(
            error.contains("sources.s.kind: a kind is a string"),
            "{error}"
        );
        assert!(
            error.contains("destinations.d: give its kind: kind = one of \"discord\""),
            "{error}"
        );
        assert!(
            error.contains("markers.key: a secret is written as"),
            "{error}"
        );
        let extra = EXAMPLE.replace(
            "kind = \"git-branch\"",
            "kind = \"git-branch\"\nbranch_name = \"x\"",
        );
        assert!(errors(&extra).contains("store.branch_name: unknown field `branch_name`"));
        let stray = EXAMPLE.replace(
            "kind = \"git-branch\"",
            "kind = \"markers\"\nbranch = \"x\"",
        );
        assert!(errors(&stray).contains("store.branch: unknown field `branch`"));
        let markers = EXAMPLE.replace("kind = \"git-branch\"", "kind = \"markers\"");
        assert_eq!(
            load("herald.toml", &markers, &nothing).unwrap().store,
            Store::Markers
        );
        let pinned = EXAMPLE.replace("crosspost = true", "crosspost = true\npin = \"store\"");
        assert_eq!(
            load("herald.toml", &pinned, &nothing).unwrap().routes[0]
                .route
                .delivery
                .pin,
            Some(Pinning::Store)
        );
    }

    #[test]
    fn a_mistake_inside_a_section_is_named_by_its_key_path() {
        let text = EXAMPLE
            .replace("kinds = [\"release\"]", "kinds = [\"release\", \"tweet\"]")
            .replace("archive_after = \"14d\"", "archive_after = \"14s\"");
        let error = errors(&text);
        assert!(
            error.contains("routes[0].kinds[1]: unknown variant `tweet`"),
            "{error}"
        );
        assert!(error.contains("routes[0].thread.archive_after: invalid value: string \"14s\", expected a length of time such as \"30m\""), "{error}");
        let unnamed = EXAMPLE.replace(
            "thread = { name = \"{{ event.project }} {{ event.version }}\", archive_after = \"14d\" }",
            "thread = { archive_after = \"14d\" }",
        );
        assert!(errors(&unnamed).contains("routes[0].thread: missing field `name`"));
        let listed = EXAMPLE.replace(
            "thread = { name = \"{{ event.project }} {{ event.version }}\", archive_after = \"14d\" }",
            "thread = [\"a\"]",
        );
        let error = errors(&listed);
        assert!(
            error.contains("routes[0].thread: invalid length 1, expected struct Thread"),
            "{error}"
        );
        assert!(
            errors(&EXAMPLE.replace("version = 1", "version = \"1\""))
                .contains("version: invalid type: string")
        );
        let untemplated = EXAMPLE.replace("template = \"package-release\"\n", "");
        assert!(errors(&untemplated).contains("routes[0]: missing field `template`"));
    }

    #[test]
    fn a_route_without_kinds_is_dry_rendered_with_its_sources_kinds() {
        let text = EXAMPLE.replace("kinds = [\"release\"]\n", "").replace(
            "body = \"{{ event.body }}\"",
            "body = \"{{ event.version | upper }}\"",
        );
        let config = load("herald.toml", &text, &nothing).unwrap();
        assert_eq!(config.routes[0].route.filter.kinds, Vec::<Kind>::new());
        assert_eq!(
            config.sources.values().next().unwrap().kinds(),
            [Kind::Release]
        );
    }

    #[test]
    fn a_section_of_the_wrong_shape_or_with_a_stray_key_is_named() {
        let text = format!(
            "{EXAMPLE}\n[apps.ci]\ntoken = \"x\"\n[templates.t]\ntitle = \"t\"\nbody = \"b\"\nextra = 1\n"
        );
        let error = errors(&text);
        assert!(
            error.contains("apps.ci.token: unknown field `token`"),
            "{error}"
        );
        assert!(
            error.contains("templates.t.extra: unknown field `extra`"),
            "{error}"
        );
        let error = errors("version = 1\nsources = 3\n[store]\nkind = \"markers\"\n");
        assert!(
            error.contains("sources: sources is a table; write [sources]"),
            "{error}"
        );
    }

    #[test]
    fn the_committed_schema_is_the_one_the_types_generate() {
        assert_eq!(
            schema(),
            include_str!("../../../herald.schema.json"),
            "regenerate it: cargo run -p herald-config --example schema > herald.schema.json"
        );
    }
}
