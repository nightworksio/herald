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
    assert_eq!(announcement.title, "📦 php-mutation-gate v1");
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
