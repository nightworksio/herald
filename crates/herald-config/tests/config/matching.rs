//! A route's `match`, an owner's member overrides naming routes, and an
//! app's project globs, read with their defaults, and every mistake in
//! them named by key.

use herald_config::load;
use herald_core::{AreaPaths, Glob, Reach, RouteId};
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
private = false
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
fn a_match_reads_the_attributes_it_narrows_by_and_what_it_never_takes() {
    let config = loaded(&route(
            "tags",
            "\"org\"",
            "t",
            "match = { attributes = { signal = [\"tag\"] }, not = { projects = [\"web*\"], attributes = { prerelease = [\"true\"] } } }",
        ))
        .unwrap();
    let matching = &config.routes[1].route.filter.matching;
    assert_eq!(matching.attributes["signal"], ["tag"]);
    let not = matching.not.as_deref().unwrap();
    assert_eq!(not.projects, [Glob::new("web*").unwrap()]);
    assert_eq!(not.attributes["prerelease"], ["true"]);
    assert_eq!(not.not, None);
}

#[test]
fn a_not_narrowing_by_any_one_key_is_read() {
    for key in [
        "projects = [\"web*\"]",
        "repositories = [\"lemonfiber/web\"]",
        "owners = [\"nightworksio\"]",
        "forges = [\"gitlab\"]",
        "hosts = [\"ghe.example\"]",
        "topics = [\"archive\"]",
        "properties = { tier = [] }",
        "attributes = { signal = [\"tag\"] }",
    ] {
        let extra = format!("match = {{ not = {{ {key} }} }}");
        let config = loaded(&route("tags", "\"org\"", "t", &extra));
        assert!(config.is_ok(), "{key}: {config:?}");
    }
}

#[test]
fn an_attribute_no_kind_carries_and_a_not_that_takes_everything_are_named() {
    for (extra, key, said) in [
        (
            "match = { attributes = { conclusion = [\"failure\"] } }",
            "routes[1].match.attributes.conclusion",
            "no kind the route takes carries an attribute \"conclusion\"; its kinds carry \"edited\", \"prerelease\", \"signal\", \"version\"",
        ),
        (
            "match = { not = { attributes = { conclusion = [] } } }",
            "routes[1].match.not.attributes.conclusion",
            "no kind the route takes carries an attribute \"conclusion\"",
        ),
        (
            "match = { not = {} }",
            "routes[1].match.not",
            "not narrows by nothing, so it would take every event; name what is never taken, or remove it",
        ),
        (
            "match = { not = { attributes = {} } }",
            "routes[1].match.not",
            "not narrows by nothing",
        ),
        (
            "match = { not = { projects = [\"a\"], not = { projects = [\"b\"] } } }",
            "routes[1].match.not",
            "a not holds no not of its own; write what it never takes beside it",
        ),
        (
            "match = { not = { projects = [\"[\"] } }",
            "routes[1].match.not.projects[0]",
            "",
        ),
    ] {
        let error = loaded(&route("tags", "\"org\"", "t", extra)).unwrap_err();
        assert!(error.contains(key), "{key}: {error}");
        assert!(error.contains(said), "{said}: {error}");
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

const AREAS: &str = "[sources.site]\nkind = \"github\"\nrepository = \"nightworksio/site\"\n\n[sources.site.areas]\nweb = [\"web/**\"]\ndocs = [\"*.md\"]\n\n";

#[test]
fn a_route_narrows_by_the_areas_its_sources_name_read_where_it_says() {
    let config = loaded(&format!(
        "{AREAS}{}{}{}",
        route(
            "web",
            "\"site\"",
            "t",
            "match = { areas = [\"WEB\"], not = { areas = [\"d*\"] } }"
        ),
        route(
            "read",
            "\"site\", \"gate\"",
            "t",
            "area_paths = \"delivery\"\nmatch = { areas = [\"docs\"] }"
        ),
        route("all", "\"site\"", "t", "")
    ))
    .unwrap();
    let web = &config.routes[1].route.filter;
    assert_eq!(web.matching.areas, [Glob::new("WEB").unwrap()]);
    assert_eq!(
        web.matching.not.as_deref().unwrap().areas,
        [Glob::new("d*").unwrap()]
    );
    assert_eq!(web.area_paths, AreaPaths::Kept);
    let read = &config.routes[2].route.filter;
    assert_eq!(read.matching.areas, [Glob::new("docs").unwrap()]);
    assert_eq!(read.area_paths, AreaPaths::Delivery);
    assert_eq!(config.routes[3].route.filter.area_paths, AreaPaths::Kept);
    assert!(!config.routes[3].route.filter.matching.reads_areas());
}

#[test]
fn a_route_s_area_its_sources_do_not_name_and_a_read_at_delivery_for_no_area_are_refused() {
    let error = loaded(&format!(
        "{AREAS}{}{}{}",
        route(
            "web",
            "\"site\"",
            "t",
            "match = { areas = [\"web\", \"api\"], not = { areas = [\"[\"] } }"
        ),
        route("bare", "\"gate\"", "t", "match = { areas = [\"web\"] }"),
        route("idle", "\"site\"", "t", "area_paths = \"delivery\"")
    ))
    .unwrap_err();
    for wanted in [
        "routes[1].match.areas[1]: \"api\" matches no area the route's sources name; the areas are \"docs\", \"web\"",
        "routes[1].match.not.areas[0]",
        "routes[2].match.areas[0]: \"web\" matches no area the route's sources name; they name none; give a source an areas table",
        "routes[3].area_paths: a route reads paths at delivery only for a match naming areas",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
    assert!(!error.contains("routes[1].match.areas[0]"), "{error}");
}
