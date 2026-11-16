//! A route's `match`, an owner's member overrides naming routes, and an
//! app's project globs, read with their defaults, and every mistake in
//! them named by key.

mod common;

#[cfg(test)]
mod matching {
    use herald_config::load;
    use herald_core::{Glob, Reach, RouteId};
    use herald_sources_forge::ForgeSource;

    const HEAD: &str = r#"version = 1

[store]
kind = "sqlite"

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.org]
kind = "github"
owner = "lemonfiber"

[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[apps.ci]
tokens = ["sha256:1111111111111111111111111111111111111111111111111111111111111111"]
projects = ["plugin-*", "deploys"]

[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"

[templates.origin]
title = "{{ event.origin.repository }}"
body = "{{ event.body }}"

[[routes]]
id = "deploys"
sources = ["ci"]
destination = "discord"
channel = "1566200000000000900"
template = "t"
"#;

    fn nothing(_: &str) -> Result<String, String> {
        Err("no file".to_owned())
    }

    fn loaded(rest: &str) -> Result<herald_config::Config, String> {
        load(
            "herald.toml",
            &format!("{HEAD}\n{rest}"),
            &nothing,
            &crate::common::kinds(),
        )
        .map_err(|error| error.to_string())
    }

    fn route(id: &str, sources: &str, template: &str, extra: &str) -> String {
        format!(
            "[[routes]]\nid = \"{id}\"\nsources = [{sources}]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"{template}\"\n{extra}\n"
        )
    }

    #[test]
    fn a_route_reads_its_match_and_matches_everything_where_it_gives_none() {
        let config = loaded(&format!(
            "{}{}",
            route(
                "plugins",
                "\"org\"",
                "t",
                "match = { projects = [\"plugin-*\"], repositories = [\"*/plugin-*\"], owners = [\"lemon*\"], forges = [\"github\"], hosts = [\"github.com\"], topics = [\"media\"], properties = { team = [\"core\"] } }"
            ),
            route("all", "\"org\"", "t", "")
        ))
        .unwrap();
        let matching = &config.routes[1].route.filter.matching;
        assert_eq!(matching.projects, [Glob::new("plugin-*").unwrap()]);
        assert_eq!(matching.repositories, [Glob::new("*/plugin-*").unwrap()]);
        assert_eq!(matching.owners, [Glob::new("lemon*").unwrap()]);
        assert_eq!(matching.forges, ["github"]);
        assert_eq!(matching.hosts, ["github.com"]);
        assert_eq!(matching.topics, ["media"]);
        assert_eq!(matching.properties["team"], ["core"]);
        assert_eq!(
            config.routes[2].route.filter.matching,
            herald_core::Match::default()
        );
    }

    #[test]
    fn each_glob_of_a_match_that_cannot_be_read_is_named_by_key() {
        let error = loaded(&route(
            "plugins",
            "\"org\"",
            "t",
            "match = { projects = [\"[\"], repositories = [\"a\", \"[\"], owners = [\"[\"] }",
        ))
        .unwrap_err();
        for key in [
            "routes[1].match.projects[0]",
            "routes[1].match.repositories[1]",
            "routes[1].match.owners[0]",
        ] {
            assert!(error.contains(key), "{key}: {error}");
        }
    }

    #[test]
    fn a_member_override_reads_the_routes_it_names_and_whether_they_are_its_only_ones() {
        let config = loaded(&format!(
            "{}{}[sources.org.repositories.\"plugin-*\"]\nroutes = [\"plugins\"]\nroutes_only = true\n",
            route("all", "\"org\"", "t", ""),
            route("plugins", "\"gate\"", "t", "")
        ))
        .unwrap();
        let source = &config.sources[&herald_core::SourceName::new("org")];
        let forged = source.of::<ForgeSource>().unwrap();
        assert_eq!(
            forged.options.reach_of("plugin-sonarr"),
            Reach {
                routes: vec![RouteId::new("plugins")],
                only: true,
            }
        );
        assert_eq!(forged.options.reach_of("website"), Reach::default());
        assert_eq!(
            source.routed(),
            [(
                "repositories.\"plugin-*\".routes".to_owned(),
                "plugins".to_owned()
            )]
        );
        assert!(source.origins());
    }

    #[test]
    fn an_override_naming_no_route_or_a_public_one_for_private_members_does_not_load() {
        let error = loaded(&format!(
            "{}[sources.org.repositories.\"plugin-*\"]\nroutes = [\"nowhere\"]\n",
            route("all", "\"org\"", "t", "")
        ))
        .unwrap_err();
        assert!(
            error.contains("sources.org.repositories.\"plugin-*\".routes: there is no route with the id \"nowhere\""),
            "{error}"
        );
        let private = "[sources.org]\nkind = \"github\"\nowner = \"lemonfiber\"\nvisibility = [\"public\", \"private\"]\n";
        let unlisted = HEAD.replace(
            "[sources.org]\nkind = \"github\"\nowner = \"lemonfiber\"\n",
            private,
        );
        let text = format!(
            "{unlisted}\n{}{}[sources.org.repositories.\"plugin-*\"]\nroutes = [\"public\"]\n",
            route("all", "\"org\"", "t", "private = true"),
            route("public", "\"gate\"", "t", "")
        );
        let error = load("herald.toml", &text, &nothing, &crate::common::kinds())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("the route public is not declared private"),
            "{error}"
        );
        let warned = format!("{text}\n[doctor]\nprivate_to_public = \"warn\"\n");
        let warned = load("herald.toml", &warned, &nothing, &crate::common::kinds()).map(drop);
        assert_eq!(warned, Ok(()));
    }

    #[test]
    fn an_app_s_projects_are_globs_and_only_those_without_a_wildcard_are_its_own() {
        let config = loaded("").unwrap();
        let app = &config.apps["ci"];
        assert!(app.announces("plugin-sonarr"));
        assert!(app.announces("Deploys"));
        assert!(!app.announces("herald"));
        assert_eq!(app.named().collect::<Vec<_>>(), ["deploys"]);
        let bad = HEAD.replace(
            "projects = [\"plugin-*\", \"deploys\"]",
            "projects = [\"[\"]",
        );
        let error = load("herald.toml", &bad, &nothing, &crate::common::kinds())
            .unwrap_err()
            .to_string();
        assert!(error.contains("apps.ci.projects[0]"), "{error}");
    }

    #[test]
    fn a_template_reading_the_origin_loads_only_where_every_event_the_route_takes_has_one() {
        assert!(loaded(&route("forges", "\"org\", \"gate\"", "origin", "")).is_ok());
        let error = loaded(&route("apps", "\"ci\"", "origin", "")).unwrap_err();
        assert!(
            error.contains("templates.origin.title: it does not render a sample"),
            "{error}"
        );
    }
}
