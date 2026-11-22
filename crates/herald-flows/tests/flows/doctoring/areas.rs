//! What doctor says of each route narrowing by areas: where its events'
//! paths are read, what that costs, and when an event has none (ADR-0072).

use herald_flows::Here;

use super::{loaded, on, server};

fn area_notes(view: &str, paths: &str) -> Vec<String> {
    let config = loaded(&format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.site]
kind = "github"
repository = "o/site"
signals = ["pull-request"]
[sources.site.areas]
web = ["web/**"]
[[routes]]
id = "web"
sources = ["site"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
{paths}
match = {{ areas = ["web"] }}
[[routes]]
id = "all"
sources = ["site"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
{view}
"#
    ));
    on(&config, &server(), &Here::default())
        .notes
        .into_iter()
        .filter(|note| note.starts_with("The route"))
        .collect()
}

#[test]
fn a_route_narrowing_by_kept_areas_says_which_sources_no_view_reads_kept() {
    let unkept = area_notes("", "");
    assert_eq!(unkept.len(), 1, "{unkept:?}");
    for wanted in [
        "The route web takes each pull request event's areas from the paths kept for that pull request where its kept head is the event's, no request more",
        "no view reads site kept, so its events fall in no area: add a kept view of it, or set area_paths = \"delivery\"",
        "falls in no area, so match.areas does not take it and not.areas refuses it.",
    ] {
        assert!(unkept[0].contains(wanted), "{wanted}\n{}", unkept[0]);
    }
    let kept = area_notes("[views.board]\nsources = [\"site\"]\nmode = \"kept\"\n", "");
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(!kept[0].contains("no view reads"), "{}", kept[0]);
}

#[test]
fn a_route_reading_paths_at_delivery_names_the_request_each_event_costs() {
    let read = area_notes("", "area_paths = \"delivery\"");
    assert_eq!(read.len(), 1, "{read:?}");
    assert!(
        read[0].starts_with("The route web reads the paths each pull request event's pull request changes from the forge as the event is delivered: one request more for each"),
        "{}",
        read[0]
    );
}
