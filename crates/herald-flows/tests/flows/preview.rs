use herald_core::RouteId;
use herald_flows::preview;

use crate::harness::*;

#[test]
fn each_route_renders_the_event_as_its_destination_would_show_it() {
    let config = config("");
    let previewed = preview(&config, &event("v1"), None);
    assert_eq!(previewed.len(), 1);
    assert_eq!(previewed[0].route, RouteId::new("releases"));
    assert_eq!(previewed[0].destination.as_str(), "discord");
    let announcement = previewed[0].rendered.as_ref().unwrap();
    assert_eq!(announcement.title.plain(), "📦 php-mutation-gate v1");
    assert_eq!(announcement.thread.as_deref(), Some("v1"));
    assert_eq!(announcement.parts[0].body, "## Added");
    assert_eq!(
        preview(&config, &event("v1"), Some(&RouteId::new("elsewhere"))),
        []
    );
    assert_eq!(
        preview(&config, &event("v1"), Some(&RouteId::new("releases"))).len(),
        1
    );
}

#[test]
fn an_event_a_template_cannot_render_says_why() {
    let previewed = preview(&config(""), &event("boom"), None);
    let error = previewed[0].rendered.as_ref().unwrap_err();
    assert!(error.contains("undefined value"), "{error}");
}

#[test]
fn each_route_that_groups_renders_the_events_as_one_group_led_by_its_lead() {
    use crate::grouping::{grouped, release};
    let config = grouped("", "");
    let events = [release("cli", 2), release("core", 1)];
    let previewed = herald_flows::preview_group(&config, ("v9", &events), None);
    assert_eq!(previewed.len(), 1);
    assert_eq!(previewed[0].route, RouteId::new("train"));
    let announcement = previewed[0].rendered.as_ref().unwrap();
    assert_eq!(announcement.title.plain(), "v9 of 2");
    assert_eq!(announcement.project, "core");
    assert!(
        announcement.parts[0]
            .body
            .starts_with("core with notes and cli with notes"),
        "{}",
        announcement.parts[0].body
    );
    let only = Some(RouteId::new("train"));
    assert_eq!(
        herald_flows::preview_group(&config, ("v9", &events), only.as_ref()).len(),
        1
    );
    let elsewhere = Some(RouteId::new("elsewhere"));
    assert_eq!(
        herald_flows::preview_group(&config, ("v9", &events), elsewhere.as_ref()),
        []
    );
    assert_eq!(herald_flows::preview_group(&config, ("v9", &[]), None), []);
    assert_eq!(
        herald_flows::preview_group(&crate::harness::config(""), ("v9", &events), None),
        []
    );
    let alone = preview(&config, &events[0], None);
    assert_eq!(
        alone[0].rendered.as_ref().unwrap().title.plain(),
        "cli alone"
    );
}
