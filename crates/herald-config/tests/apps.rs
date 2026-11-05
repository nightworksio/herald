//! `[apps]`: the HTTP API's apps, each a source routes may name, read with
//! their tokens' hashes, and every mistake in them named by key.

#[cfg(test)]
mod apps {
    use herald_config::load;
    use herald_core::Kind;

    const EXAMPLE: &str = r#"version = 1

[store]
kind = "git-branch"

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "releases"
sources = ["gate"]
destination = "discord"
channel = "100"
template = "t"

[[routes]]
id = "deploys"
sources = ["ci"]
destination = "discord"
channel = "200"
template = "t"

[[routes]]
id = "everything"
sources = ["gate", "ci"]
destination = "discord"
channel = "300"
template = "t"

[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#;

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn token(digit: char) -> String {
        format!("sha256:{}", digit.to_string().repeat(64))
    }

    fn with(apps: &str) -> String {
        format!("{EXAMPLE}\n{apps}")
    }

    fn errors(text: &str) -> String {
        load("herald.toml", text, &nothing).unwrap_err().to_string()
    }

    #[test]
    fn an_app_reads_its_tokens_projects_and_every_route_that_names_it() {
        let text = with(&format!(
            "[apps.ci]\ntokens = [\"{}\", \"{}\"]\nprojects = [\"herald\"]\n",
            token('0'),
            token('f')
        ));
        let config = load("herald.toml", &text, &nothing).unwrap();
        let app = &config.apps["ci"];
        assert_eq!(app.tokens, ["0".repeat(64), "f".repeat(64)]);
        assert_eq!(app.projects, ["herald"]);
        let routes: Vec<&str> = app
            .routes
            .iter()
            .map(herald_core::RouteId::as_str)
            .collect();
        assert_eq!(routes, ["deploys", "everything"]);
    }

    #[test]
    fn an_app_naming_its_routes_announces_to_those_only() {
        let text = with(&format!(
            "[apps.ci]\ntokens = [\"{}\"]\nroutes = [\"everything\"]\n",
            token('c')
        ));
        let config = load("herald.toml", &text, &nothing).unwrap();
        let routes: Vec<&str> = config.apps["ci"]
            .routes
            .iter()
            .map(herald_core::RouteId::as_str)
            .collect();
        assert_eq!(routes, ["everything"]);
    }

    #[test]
    fn an_app_with_no_tokens_is_read_for_the_cli_alone() {
        let config = load("herald.toml", &with("[apps.ci]\n"), &nothing).unwrap();
        assert_eq!(config.apps["ci"].tokens, Vec::<String>::new());
    }

    #[test]
    fn a_route_naming_an_app_takes_its_messages() {
        let config = load("herald.toml", &with("[apps.ci]\n"), &nothing).unwrap();
        let deploys = config
            .routes
            .iter()
            .find(|route| route.route.id.as_str() == "deploys")
            .unwrap();
        assert!(
            deploys
                .route
                .filter
                .sources
                .iter()
                .any(|source| source.as_str() == "ci")
        );
        let kinds = with("[apps.ci]\n").replace(
            "id = \"deploys\"\nsources = [\"ci\"]",
            "id = \"deploys\"\nsources = [\"ci\"]\nversions = \"1.*\"",
        );
        assert!(
            errors(&kinds)
                .contains("routes[1].versions: feed entries and app messages carry no version"),
            "{}",
            errors(&kinds)
        );
        assert!(!Kind::Message.versioned());
    }

    #[test]
    fn a_token_not_written_as_herald_token_new_prints_it_is_refused() {
        for written in [
            "a".repeat(64),
            "sha256:ABC".to_owned(),
            format!("sha256:{}", "g".repeat(64)),
            format!("sha256:{}", "a".repeat(63)),
            format!("sha1:{}", "a".repeat(64)),
        ] {
            let error = errors(&with(&format!("[apps.ci]\ntokens = [\"{written}\"]\n")));
            assert!(
                error.contains(
                    "apps.ci.tokens: a token is written sha256: and its SHA-256 in 64 lowercase hex digits, as herald token new prints it"
                ),
                "{written}: {error}"
            );
        }
    }

    #[test]
    fn an_app_named_as_a_source_is_refused() {
        let error = errors(&EXAMPLE.replace("[templates.t]", "[apps.gate]\n\n[templates.t]"));
        assert!(
            error.contains("apps.gate: a source is named \"gate\" too; a route names sources and apps alike, so give each its own name"),
            "{error}"
        );
    }

    #[test]
    fn an_app_naming_a_route_that_does_not_take_it_or_does_not_exist_is_refused() {
        let error = errors(&with("[apps.ci]\nroutes = [\"releases\", \"nowhere\"]\n"));
        assert!(
            error.contains(
                "apps.ci.routes: the route \"releases\" takes no event of \"ci\"; name the app in its sources"
            ),
            "{error}"
        );
        assert!(
            error.contains("apps.ci.routes: no route has the id \"nowhere\""),
            "{error}"
        );
    }

    #[test]
    fn a_route_naming_neither_a_source_nor_an_app_names_both_kinds() {
        let error = errors(EXAMPLE);
        assert!(
            error.contains(
                "routes[1].sources: no source or app is named \"ci\"; the sources and apps are \"gate\""
            ),
            "{error}"
        );
        let error =
            errors(&with("[apps.ci]\n").replace("sources = [\"gate\"]", "sources = [\"nobody\"]"));
        assert!(
            error.contains("the sources and apps are \"ci\", \"gate\""),
            "{error}"
        );
    }
}
