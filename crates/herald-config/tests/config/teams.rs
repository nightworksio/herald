//! `[teams]` and a view's `load`: the teams a load counts, and how it
//! folds, every mistake named by key (ADR-0075).

use std::fmt::Write as _;

use herald_config::{Config, load};
use herald_core::{LEAST_TEAM, LOAD_WINDOW, Load, MOST_TEAMS, Team};
use jiff::SignedDuration;

const EXAMPLE: &str = r#"version = 1

[store]
kind = "sqlite"

[sources.herald]
kind = "github"
repository = "nightworksio/herald"
"#;

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn with(tables: &str) -> String {
    format!("{EXAMPLE}\n{tables}")
}

fn loaded(text: &str) -> Config {
    load("herald.toml", text, &nothing, &crate::common::kinds()).unwrap()
}

fn errors(text: &str) -> String {
    load("herald.toml", text, &nothing, &crate::common::kinds())
        .unwrap_err()
        .to_string()
}

/// `teams` empty teams, each its own table.
fn named(teams: usize) -> String {
    (0..teams).fold(String::new(), |mut text, n| {
        let _ = writeln!(text, "[teams.t{n}]\nmembers = []");
        text
    })
}

#[test]
fn a_team_names_its_members_each_a_login_or_a_login_on_one_host() {
    let config = loaded(&with(
        "[teams.\"acme/platform\"]\nmembers = [\"ada\", \"Bob\", \"cy@gitlab.example.com\"]\n\n[teams.empty]\nmembers = []\n",
    ));
    let members: Vec<String> = ["ada", "Bob", "cy@gitlab.example.com"]
        .map(str::to_owned)
        .to_vec();
    assert_eq!(
        config.teams["acme/platform"],
        Team::of("acme/platform", &members).unwrap()
    );
    assert_eq!(
        config.teams["acme/platform"].people(),
        ["ada", "bob", "cy"].into()
    );
    assert_eq!(config.teams["empty"], Team::default());
    assert!(loaded(EXAMPLE).teams.is_empty());
}

#[test]
fn a_team_that_is_no_list_of_logins_or_past_its_bounds_is_refused() {
    let error = errors(&with(
        "[teams.odd]\nmembers = [\"a b\"]\n\n[teams.slash]\nmembers = [\"acme/ada\"]\n\n[teams.extra]\nmembers = []\nrole = \"x\"\n\n[teams.\" padded\"]\nmembers = []\n",
    ));
    for wanted in [
        "teams.odd: \"a b\" holds a space, a slash or a control character",
        "teams.slash: \"acme/ada\" holds a space",
        "teams.extra",
        "teams. padded: \" padded\" is no team name",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
    let error = errors(&with(&named(MOST_TEAMS + 1)));
    assert!(
        error.contains("teams: a config names at most 200 teams"),
        "{error}"
    );
    assert_eq!(loaded(&with(&named(MOST_TEAMS))).teams.len(), MOST_TEAMS);
    let members: Vec<String> = (0..=herald_core::MOST_TEAM_MEMBERS)
        .map(|n| format!("\"m{n}\""))
        .collect();
    let error = errors(&with(&format!(
        "[teams.big]\nmembers = [{}]\n",
        members.join(", ")
    )));
    assert!(
        error.contains("teams.big: a team names at most 500 members"),
        "{error}"
    );
}

#[test]
fn a_view_shows_its_load_only_where_it_names_one_as_shipped_or_as_written() {
    let config = loaded(&with(
        "[views.plain]\n\n[views.shipped]\nload = {}\n\n[views.written]\nload = { window = \"90d\", least_team = 50, unknown_teams = true }\n\n[views.shortest]\nload = { window = \"1d\", least_team = 3 }\n",
    ));
    assert_eq!(config.views["plain"].load, None);
    assert_eq!(config.views["queue"].load, None);
    assert_eq!(
        config.views["shipped"].load,
        Some(Load {
            window: LOAD_WINDOW,
            least_team: LEAST_TEAM,
            unknown_teams: false,
        })
    );
    assert_eq!(
        config.views["written"].load,
        Some(Load {
            window: SignedDuration::from_hours(90 * 24),
            least_team: 50,
            unknown_teams: true,
        })
    );
    assert_eq!(
        config.views["shortest"].load.map(|load| load.window),
        Some(SignedDuration::from_hours(24))
    );
}

#[test]
fn a_least_team_below_three_or_past_fifty_and_a_window_past_its_bounds_are_refused() {
    let error = errors(&with(
        "[views.two]\nload = { least_team = 2 }\n\n[views.one]\nload = { least_team = 1 }\n\n[views.wide]\nload = { least_team = 51 }\n\n[views.long]\nload = { window = \"91d\" }\n\n[views.short]\nload = { window = \"23h\" }\n\n[views.odd]\nload = { people = 3 }\n",
    ));
    for wanted in [
        "views.two.load.least_team: a load never shows a team or an area of fewer than 3 people by name, so no row is one person's; give least_team = 3 or more",
        "views.one.load.least_team: a load never shows a team or an area of fewer than 3 people",
        "views.wide.load.least_team: a load's least_team is at most 50, such as least_team = 3",
        "views.long.load.window: a load counts reviews given over a window from 1d to 90d, such as window = \"7d\"",
        "views.short.load.window: a load counts reviews given over a window from 1d to 90d",
        "views.odd",
    ] {
        assert!(error.contains(wanted), "{wanted}\n{error}");
    }
}
