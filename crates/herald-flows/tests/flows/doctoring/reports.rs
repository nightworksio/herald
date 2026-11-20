//! What doctor says of each report: how its view reads each source, at
//! what cost, and which routes see its rows (ADR-0055).

use herald_config::Config;
use herald_flows::Here;

use super::{loaded, on, server};

fn reporting(doctor: &str) -> Config {
    loaded(&format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.graphed]
kind = "github"
repository = "o/graphed"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[sources.bare]
kind = "github"
repository = "o/bare"
[sources.forgejo]
kind = "forgejo"
repository = "o/forgejo"
api = "https://codeberg.org/api/v1"
[views.mine]
most_pulls = 20
[reports.daily]
view = "mine"
zone = "Europe/Amsterdam"
[reports.plain]
[[routes]]
id = "public"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:queue"
private = false
[[routes]]
id = "team"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000200"
template = "herald:queue"
private = true
{doctor}
"#
    ))
}

fn report_notes(config: &Config) -> Vec<String> {
    on(config, &server(), &Here::default())
        .notes
        .into_iter()
        .filter(|note| note.starts_with("The report") || note.starts_with("The route"))
        .collect()
}

#[test]
fn each_report_names_how_its_view_reads_each_source_at_what_cost_and_who_sees_its_rows() {
    assert_eq!(
        report_notes(&reporting("")),
        [
            "The report daily reads the view mine of bare by rest, at most 45 requests a round; its open pull requests past the first 20 read as unknown; GitHub's REST API reads no mergeability, so none of its bots' batches counts green; give the source a token to read by GraphQL.",
            "The report daily reads the view mine of forgejo by rest, at most 45 requests a round; its open pull requests past the first 20 read as unknown.",
            "The report daily reads the view mine of graphed by graphql, at most 5 requests a round.",
            "The report daily reads its times in Europe/Amsterdam from the time zone database bundled into this build, about 207 KB (49 KB compressed).",
            "The route public is not private, so it is shown the report daily's rows of public repositories alone, and nothing where the view reads none.",
            "The report plain reads the view queue of bare by rest, at most 205 requests a round; its open pull requests past the first 100 read as unknown; GitHub's REST API reads no mergeability, so none of its bots' batches counts green; give the source a token to read by GraphQL.",
            "The report plain reads the view queue of forgejo by rest, at most 205 requests a round; its open pull requests past the first 100 read as unknown.",
            "The report plain reads the view queue of graphed by graphql, at most 5 requests a round.",
        ]
    );
    let warned = report_notes(&reporting("[doctor]\nprivate_to_public = \"warn\""));
    assert!(
        warned.contains(&"The route public is not private and is shown the report daily's rows of private and internal repositories too, as [doctor] private_to_public = \"warn\" lets it.".to_owned()),
        "{warned:?}"
    );
    let allowed = report_notes(&reporting("[doctor]\nprivate_to_public = \"allow\""));
    assert!(
        !allowed.iter().any(|note| note.starts_with("The route")),
        "{allowed:?}"
    );
}

#[test]
fn each_rule_names_its_reads_and_stages_and_a_living_route_that_cannot_edit_is_named() {
    let config = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.slack]
kind = "slack"
webhook = { secret = "HERALD_SLACK_WEBHOOK" }
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.graphed]
kind = "github"
repository = "o/graphed"
token = { secret = "HERALD_GITHUB_TOKEN" }
[rules.waiting]
stages = ["2d", "1w"]
most = 5
zone = "Europe/Amsterdam"
[boards.hooked]
destination = "slack"
private = true
[boards.edited]
destination = "discord"
channel = "1566200000000000100"
private = true
[[routes]]
id = "nudges"
sources = ["waiting"]
destination = "slack"
template = "herald:nudge"
private = false
"#,
    );
    let notes: Vec<String> = on(&config, &server(), &Here::default())
        .notes
        .into_iter()
        .filter(|note| note.contains("rule") || note.contains("living"))
        .collect();
    assert_eq!(
        notes,
        [
            "The rule waiting reads the view queue of graphed by graphql, at most 5 requests a round.",
            "The rule waiting reads its times in Europe/Amsterdam from the time zone database bundled into this build, about 207 KB (49 KB compressed).",
            "The route nudges is not private, so it is shown the rule waiting's rows of public repositories alone, and nothing where the view reads none.",
            "The rule waiting nudges at most 5 pull requests a window, once each past 2d, 1w, naming logins without mentioning anyone.",
            "The route hooked is living, but its destination cannot edit a post, so each window of its report is posted anew.",
        ]
    );
}

#[test]
fn a_view_of_an_owner_names_its_scope_query_and_one_taking_listed_visibility_says_what_it_widens() {
    let config = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[sources.org]
kind = "github"
owner = "o"
token = { secret = "HERALD_GITHUB_TOKEN" }
[views.queue]
listed_visibility = true
[reports.daily]
"#,
    );
    let notes = on(&config, &server(), &Here::default()).notes;
    let named = |start: &str| {
        notes
            .iter()
            .find(|note| note.starts_with(start))
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(
        named("The report daily"),
        "The report daily reads the view queue of org by search, at most 10 requests a round; it reads only the repositories the owner's membership record holds, each seen as that record says; where GitHub's search finds more than 1000, or the owner is a GitLab user, it lists each repository on its own, at least a request each."
    );
    assert_eq!(
        named("The view queue takes"),
        "The view queue takes who may see each repository a REST listing reads without a token, on GitHub, GitHub Enterprise Server, GHE.com or Forgejo, from what the listing states, as listed_visibility = true asks; a read with a token, a GitLab listing, a kept record and an owner's rows are seen as before."
    );
}
