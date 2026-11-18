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
