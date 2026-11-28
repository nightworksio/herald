use herald_config::{Config, load};
use herald_core::{RouteId, SourceName};
use herald_flows::{Unmapped, delivered, preview_delivered, preview_delivered_group};

/// Two sources of GitHub, cli/cli and nightworksio/app, each with a route
/// of its own, each route as `extra` adds.
fn config(extra: &str) -> Config {
    let route = |id: &str, source: &str| {
        format!(
            "[[routes]]\nid = \"{id}\"\nsources = [\"{source}\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"shown\"\n{extra}"
        )
    };
    let text = format!(
        "version = 1\n[store]\nkind = \"sqlite\"\n[destinations.discord]\nkind = \"discord\"\ntoken = {{ secret = \"HERALD_DISCORD_TOKEN\" }}\n[sources.cli]\nkind = \"github\"\nrepository = \"cli/cli\"\n[sources.app]\nkind = \"github\"\nrepository = \"nightworksio/app\"\n{}{}[templates.shown]\ntitle = \"{{{{ event.project }}}}\"\nbody = \"{{{{ events | length }}}}\"\n",
        route("clis", "cli"),
        route("apps", "app"),
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

const RELEASE: (&str, &str) = ("X-GitHub-Event", "release");

#[test]
fn each_source_reads_a_delivery_as_its_webhook_does_and_says_why_it_reads_no_event() {
    let config = config("");
    let payload = crate::serving::EVENT.as_bytes();
    let events = delivered(&config, (&[RELEASE], payload)).unwrap();
    assert_eq!(
        events
            .iter()
            .map(|(source, event)| (source.as_str(), event.project()))
            .collect::<Vec<_>>(),
        [("cli", "cli")]
    );
    assert_eq!(
        delivered(&config, (&[("X-GitHub-Event", "pull_request")], payload)),
        Err(vec![
            (SourceName::new("app"), Unmapped::Unread),
            (SourceName::new("cli"), Unmapped::Unread),
        ])
    );
}

#[test]
fn a_route_renders_a_delivered_event_only_from_a_source_it_takes() {
    let config = config("");
    let events = delivered(&config, (&[RELEASE], crate::serving::EVENT.as_bytes())).unwrap();
    let shown = preview_delivered(&config, &events, None);
    assert_eq!(
        shown
            .iter()
            .map(|shown| shown.route.as_str())
            .collect::<Vec<_>>(),
        ["clis"]
    );
    assert_eq!(
        preview_delivered(&config, &events, Some(&RouteId::new("apps"))),
        []
    );
    let grouping = self::config("group = { key = \"{{ event.project }}\" }\n");
    let grouped = preview_delivered_group(&grouping, ("cli", &events), None);
    assert_eq!(
        grouped
            .iter()
            .map(|shown| (
                shown.route.as_str(),
                shown
                    .rendered
                    .as_ref()
                    .map(|announced| announced.parts[0].body.clone())
            ))
            .collect::<Vec<_>>(),
        [("clis", Ok("1".to_owned()))]
    );
}
