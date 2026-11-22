//! What doctor says of each route narrowing by areas: where its events'
//! paths are read, what that costs, and when an event has none (ADR-0072).

use herald_flows::Here;

use super::{loaded, on, server};

fn area_notes(view: &str, paths: &str) -> Vec<String> {
    notes_of(
        "[sources.site.areas]\nweb = [\"web/**\"]",
        view,
        paths,
        "The route",
    )
}

/// The notes starting `start` doctor says of a config whose source `site`
/// names its areas by `areas`.
fn notes_of(areas: &str, view: &str, paths: &str, start: &str) -> Vec<String> {
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
{areas}
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
        .filter(|note| note.starts_with(start))
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

#[test]
fn a_route_of_a_codeowners_source_names_the_file_s_request_and_the_source_where_it_reads() {
    let areas = "areas = \"codeowners\"";
    for paths in ["", "area_paths = \"delivery\""] {
        let routed = notes_of(areas, "", paths, "The route");
        assert_eq!(routed.len(), 1, "{routed:?}");
        assert!(
            routed[0].contains(
                "; a source of it reads areas from CODEOWNERS: one request more for each event"
            ),
            "{}",
            routed[0]
        );
    }
    let plain = area_notes("", "");
    assert!(!plain[0].contains("CODEOWNERS"), "{}", plain[0]);
    let sourced = notes_of(areas, "", "", "The source site reads areas");
    assert_eq!(sourced.len(), 1, "{sourced:?}");
    assert!(
        sourced[0].starts_with("The source site reads areas from each repository's CODEOWNERS file at the branch each pull request merges into"),
        "{}",
        sourced[0]
    );
}
