//! `herald.toml` read whole, and every mistake in it named by line, column,
//! key and fix.

#[cfg(test)]
mod load {
    use herald_config::{RouteOptions, Source, Store, load, schema};
    use herald_core::{Kind, Pinning, Prereleases};
    use herald_sources_forge::Forge;

    const EXAMPLE: &str = r#"#:schema https://raw.githubusercontent.com/nightworksio/herald/main/herald.schema.json
version = 1

[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }

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
            Store::GitBranch(herald_store_git_branch::StoreOptions {
                repository: None,
                branch: None,
                token: None,
                api: None,
                key: herald_core::SecretRef {
                    secret: "HERALD_STORE_KEY".to_owned()
                },
            })
        );
        assert_eq!(config.marker_key, None);
        assert!(
            config
                .destinations
                .values()
                .next()
                .and_then(|destination| destination.of::<herald_dest_discord::DestinationOptions>())
                .is_some_and(|options| options.token.is_some())
        );
        assert_eq!(
            config
                .destinations
                .values()
                .next()
                .map(|destination| destination.kind()),
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
            "herald.toml:20:15 routes[0].destination: no destination is named \"announcements\"; the destinations are \"discord\""
        );
        let text = EXAMPLE
            .replace(
                "sources = [\"php-mutation-gate\"]",
                "sources = [\"herald\"]",
            )
            .replace("template = \"package-release\"", "template = \"other\"");
        assert_eq!(
            errors(&text),
            "herald.toml:18:12 routes[0].sources: no source or app is named \"herald\"; the sources and apps are \"php-mutation-gate\"\nherald.toml:22:12 routes[0].template: no template is named \"other\"; the templates are \"package-release\""
        );
    }

    #[test]
    fn a_plain_string_where_a_secret_belongs_is_refused_without_repeating_it() {
        let text = EXAMPLE.replace(
            "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
            "token = \"a-token-written-plainly\"",
        );
        let error = errors(&text);
        assert!(error.starts_with("herald.toml:10:9 destinations.discord.token: a secret is written as { secret = \"NAME\" }"), "{error}");
        assert!(!error.contains("written-plainly"), "{error}");
    }

    #[test]
    fn every_mistake_is_reported_in_one_pass_in_file_order() {
        let text = "version = 2\nunknown = 1\n[store]\nkind = \"nowhere\"\n[destinations.d]\nkind = \"discord\"\n[sources.s]\nkind = \"bitbucket\"\n";
        assert_eq!(
            errors(text),
            [
                "herald.toml:1:11 version: this herald reads version 1 of the config; write version = 1",
                "herald.toml:2:1 unknown: herald.toml has no key \"unknown\"; the keys are \"version\", \"store\", \"markers\", \"destinations\", \"sources\", \"routes\", \"templates\", \"apps\", \"schedule\", \"server\", \"http\", \"render\", \"announce\"",
                "herald.toml:4:8 store.kind: there is no kind \"nowhere\"; the kinds are \"sqlite\", \"postgres\", \"d1\", \"durable-object\", \"dynamodb\", \"deno-kv\", \"fastly-kv\", \"markers\", \"git-branch\", \"actions-cache\", \"repo-variables\"",
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
        assert_eq!(
            secrets,
            ["HERALD_DISCORD_TOKEN", "HERALD_STORE_KEY", "STATE_TOKEN"]
        );
        let variables = EXAMPLE.replace(
            "kind = \"git-branch\"",
            "kind = \"repo-variables\"\ntoken = { secret = \"VARIABLES_TOKEN\" }",
        );
        let keyed =
            format!("{variables}\n[markers]\nkey = {{ secret = \"HERALD_MARKER_SECRET\" }}\n");
        let config = load("herald.toml", &keyed, &|_| Err(String::new())).unwrap();
        assert!(config.secrets().contains("VARIABLES_TOKEN"));
        assert!(config.secrets().contains("HERALD_MARKER_SECRET"));
        let marked = EXAMPLE.replace(
            "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }",
            "kind = \"markers\"",
        );
        let config = load("herald.toml", &marked, &|_| Err(String::new())).unwrap();
        assert_eq!(config.secrets().len(), 1);
    }

    #[test]
    fn every_store_kind_loads_as_itself() {
        for kind in Store::KINDS {
            let options = match kind {
                "repo-variables" => {
                    "\ntoken = { secret = \"HERALD_STATE_TOKEN\" }\nkey = { secret = \"HERALD_STORE_KEY\" }"
                }
                "git-branch" | "actions-cache" => "\nkey = { secret = \"HERALD_STORE_KEY\" }",
                "postgres" => "\nurl = { secret = \"HERALD_POSTGRES_URL\" }",
                _ => "",
            };
            let text = EXAMPLE.replace(
                "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }",
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
                let package = match kind {
                    "crates-io" => "herald",
                    "npm" => "@nightworksio/herald",
                    "docker-hub" => "library/caddy",
                    _ => "nightworksio/php-mutation-gate",
                };
                text = text
                    .replace(
                        "repository = \"nightworksio/php-mutation-gate\"",
                        &format!("package = \"{package}\""),
                    )
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
            refused.starts_with("herald.toml:20:12 routes[0].versions: feed entries and app messages carry no version"),
            "{refused}"
        );
    }

    #[test]
    fn a_source_naming_what_would_move_its_path_is_refused() {
        let source = |kind: &str, options: &str| {
            EXAMPLE
                .replace("kind = \"github\"", &format!("kind = \"{kind}\""))
                .replace("repository = \"nightworksio/php-mutation-gate\"", options)
        };
        for (kind, options, said) in [
            (
                "github",
                "repository = \"a/../b\"",
                "\"a/../b\" is not a repository; give it as owner/name",
            ),
            (
                "forgejo",
                "repository = \"a/b?x=1\"",
                "\"a/b?x=1\" is not a repository",
            ),
            (
                "gitlab",
                "repository = \"group\"",
                "\"group\" is not a repository; give it as the project's full path",
            ),
            (
                "github",
                "repository = \"a/b\"\napi = \"ftp://forge.example\"",
                "\"ftp://forge.example\" is not a web address",
            ),
            (
                "crates-io",
                "package = \"a/b\"",
                "\"a/b\" is not a package; give it as the crate's name",
            ),
            ("npm", "package = \"@scope\"", "\"@scope\" is not a package"),
            (
                "packagist",
                "package = \"a/b\"\napi = \"repo.example\"",
                "\"repo.example\" is not a web address",
            ),
            (
                "feed",
                "url = \"file:///etc/passwd\"",
                "\"file:///etc/passwd\" is not a web address",
            ),
        ] {
            let refused = errors(&source(kind, options));
            assert!(
                refused.contains(&format!("sources.php-mutation-gate: {said}")),
                "{kind}: {refused}"
            );
        }
        let gitlab = source("gitlab", "repository = \"group/sub/name\"");
        assert!(load("herald.toml", &gitlab, &|_| Err(String::new())).is_ok());
    }

    #[test]
    fn a_discord_route_names_its_channel_by_id() {
        for channel in ["#releases", "100", "1566184281406177531/../1"] {
            let named = EXAMPLE.replace("1566184281406177531", channel);
            assert!(
                errors(&named).contains(&format!("routes[0]: \"{channel}\" is not a channel id; give the channel's id, 17 to 20 digits")),
                "{channel}: {}",
                errors(&named)
            );
        }
    }

    #[test]
    fn only_npm_takes_a_token_among_the_registries() {
        let registry = |kind: &str| {
            EXAMPLE
                .replace("kind = \"github\"", &format!("kind = \"{kind}\""))
                .replace(
                    "repository = \"nightworksio/php-mutation-gate\"",
                    "package = \"@nightworksio/herald\"\ntoken = { secret = \"REGISTRY_TOKEN\" }",
                )
                .replace("kinds = [\"release\"]", "kinds = [\"package-version\"]")
        };
        let config = load("herald.toml", &registry("npm"), &|_| Err(String::new())).unwrap();
        assert!(config.secrets().contains("REGISTRY_TOKEN"));
        let refused = errors(&registry("crates-io"));
        assert!(
            refused.starts_with("herald.toml:12:1 sources.php-mutation-gate.token: the registry \"crates-io\" takes no token; remove token\n"),
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

    #[test]
    fn a_template_that_does_not_compile_or_render_or_fit_is_found_at_load() {
        let broken = EXAMPLE.replace("body = \"{{ event.body }}\"", "body = \"{{ event.body \"");
        let error = errors(&broken);
        assert!(
            error.starts_with(
                "herald.toml:29:8 templates.package-release.body: the template does not compile:"
            ),
            "{error}"
        );
        let undefined = EXAMPLE.replace(
            "body = \"{{ event.body }}\"",
            "body = \"{{ event.release_notes }}\"",
        );
        let error = errors(&undefined);
        assert!(error.starts_with("herald.toml:29:8 templates.package-release.body: it does not render a sample Release event:"), "{error}");
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
        let markers = EXAMPLE.replace(
            "kind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }",
            "kind = \"markers\"",
        );
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
            .replace("archive_after = \"14d\"", "archive_after = \"14y\"");
        let error = errors(&text);
        assert!(
            error.contains("routes[0].kinds[1]: unknown variant `tweet`"),
            "{error}"
        );
        assert!(error.contains("routes[0].thread.archive_after: invalid value: string \"14y\", expected a length of time such as \"250ms\", \"30s\""), "{error}");
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
