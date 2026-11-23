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
fn a_view_names_its_owner_query_what_predicting_conflicts_and_naming_areas_cost_and_what_listed_visibility_widens()
 {
    let config = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[sources.org]
kind = "github"
owner = "o"
token = { secret = "HERALD_GITHUB_TOKEN" }
[sources.fj]
kind = "forgejo"
repository = "o/fj"
api = "https://codeberg.org/api/v1"
[sources.fj.areas]
web = ["web/**"]
[views.queue]
sources = ["org"]
listed_visibility = true
conflicts = true
[views.fj]
sources = ["fj"]
conflicts = true
[views.zoned]
sources = ["fj"]
[reports.daily]
[reports.weekly]
view = "fj"
[reports.monthly]
view = "zoned"
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
        "The report daily reads the view queue of org by search, at most 10 requests a round; it reads only the repositories the owner's membership record holds, each seen as that record says; where GitHub's search finds more than 1000, or the owner is a GitLab user, it lists each repository on its own, at least a request each; to predict conflicts it reads the paths each pull request's head changes, at most 100 each, with each pull request."
    );
    assert_eq!(
        named("The report weekly"),
        "The report weekly reads the view fj of fj by rest, at most 205 requests a round; its open pull requests past the first 100 read as unknown; to predict conflicts and name each one's areas it reads the paths each pull request's head changes, at most 100 each, by at most 100 more requests a round."
    );
    assert_eq!(
        named("The report monthly"),
        "The report monthly reads the view zoned of fj by rest, at most 205 requests a round; its open pull requests past the first 100 read as unknown; to name each pull request's areas it reads the paths each pull request's head changes, at most 100 each, by at most 100 more requests a round."
    );
    assert_eq!(
        named("The view queue takes"),
        "The view queue takes who may see each repository a REST listing reads without a token, on GitHub, GitHub Enterprise Server, GHE.com or Forgejo, from what the listing states, as listed_visibility = true asks; a read with a token, a GitLab listing, a kept record and an owner's rows are seen as before."
    );
}

#[test]
fn a_rule_planning_re_runs_names_where_its_plans_go_and_a_source_read_live() {
    let text = |private: &str| {
        format!(
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
[views.queue]
mode = "live"
[rules.flaky]
flaky = {{ rerun = true }}
[[routes]]
id = "plans"
sources = ["flaky"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:rerun"
private = {private}
"#
        )
    };
    let notes = |private: &str| -> Vec<String> {
        on(&loaded(&text(private)), &server(), &Here::default())
            .notes
            .into_iter()
            .filter(|note| {
                (note.starts_with("The rule flaky")
                    && !note.starts_with("The rule flaky reads the view"))
                    || note.starts_with("herald rerun --apply")
            })
            .collect()
    };
    assert_eq!(
        notes("true"),
        [
            "The rule flaky plans at most 20 re-runs of flaky checks a window, each once a run and head with its evidence and exact call, to its private routes alone; a rule never re-runs. A plan reads the newest 5 failed runs on a head and each one's jobs: at most 6 requests, each charged to the run's budget.",
            "The rule flaky reads graphed live, and only kept records tell a flaky candidate, so none of its checks is one.",
            format!(
                "herald rerun --apply cannot re-run the jobs graphed reads: {}.",
                herald_kinds::UNWRITTEN
            )
            .as_str()
        ]
    );
    assert!(notes("false")[0].contains("no private route names it, so its plans reach nobody"));
}

#[test]
fn a_view_showing_capacity_names_what_it_reads_what_it_costs_and_what_its_token_needs() {
    let text = |extra: &str| {
        format!(
            r#"version = 1
[store]
kind = "sqlite"
[sources.graphed]
kind = "github"
repository = "o/graphed"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[views.queue]
overviews = ["capacity"]
{extra}
[reports.daily]
"#
        )
    };
    let note = |extra: &str| -> String {
        on(&loaded(&text(extra)), &server(), &Here::default())
            .notes
            .into_iter()
            .find(|note| note.contains("capacity of"))
            .unwrap()
    };
    let shown = note("");
    assert!(
        shown.starts_with("The report daily shows the view queue's capacity of the repositories it shows a pull request of, so the runs of one with no open pull request are not counted: two requests a repository on GitHub and GitLab, one on Forgejo."),
        "{shown}"
    );
    assert!(
        shown.contains("Actions: read") && shown.contains("never counted as idle"),
        "{shown}"
    );
    let members = note("capacity_from = \"members\"\ncapacity_repositories = 7");
    assert!(
        members.contains("its owner's 7 newest pushed members"),
        "{members}"
    );
    let held =
        note("capacity_from = \"members\"\ncapacity_repositories = 7\nrepositories = [\"o/g*\"]");
    assert!(
        held.contains(
            "the first 7 by name of the members it holds, with no request for their order"
        ),
        "{held}"
    );
}

#[test]
fn a_view_showing_advisories_names_its_cost_its_permissions_and_each_route_sent_the_line() {
    let text = advised_text;
    let notes = |extra: &str| -> Vec<String> {
        on(&loaded(&text(extra)), &server(), &Here::default()).notes
    };
    let found = notes("");
    let read: Vec<&String> = found
        .iter()
        .filter(|note| note.contains("advisories of"))
        .collect();
    assert_eq!(read.len(), 2, "{found:?}");
    let owner = read[0];
    assert!(
        owner.starts_with("The report daily reads the view queue's advisories of its organization's or group's own listings"),
        "{owner}"
    );
    for wanted in [
        "its 20 newest pushed members",
        "at most 1000 advisories read",
        "herald queue only with --private",
        "in a public CI job's log --private publishes them",
        "Secret scanning alerts: read",
        "security manager",
        "Developer role on GitLab Ultimate",
        "Forgejo raises no security alerts",
        "never shown as clean",
    ] {
        assert!(owner.contains(wanted), "{wanted}\n{owner}");
    }
    let withheld: Vec<&String> = found
        .iter()
        .filter(|note| note.starts_with("The route ") && note.contains(herald_core::WITHHELD))
        .collect();
    assert_eq!(withheld.len(), 1, "{found:?}");
    assert!(withheld[0].starts_with(
        "The route lobby is not private, or posts where anyone may read it, so it is sent "
    ));
    assert!(withheld[0].contains("the report daily's advisories"));
    let shown = notes("advisories_from = \"shown\"");
    assert!(
        shown
            .iter()
            .any(|note| note.contains("advisories of the repositories it shows a pull request of")),
        "{shown:?}"
    );
    let members =
        notes("advisories_from = \"members\"\nadvisory_repositories = 7\nmost_advisories = 50");
    assert!(
        members
            .iter()
            .any(|note| note.contains("its owner's 7 newest pushed members")
                && note.contains("at most 50 advisories read")),
        "{members:?}"
    );
    let held = notes(
        "advisories_from = \"members\"\nadvisory_repositories = 7\nrepositories = [\"o/g*\"]",
    );
    assert!(
        held.iter()
            .any(|note| note.contains("the first 7 by name of the members it holds, one by one")),
        "{held:?}"
    );
}

/// A config whose view `queue` shows advisories with `extra`, reported as
/// `daily` to the private route `team` and the route `lobby`, both in one
/// channel.
fn advised_text(extra: &str) -> String {
    format!(
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
[views.queue]
overviews = ["advisories"]
{extra}
[reports.daily]
[rules.nudge]
stages = ["1d"]
[[routes]]
id = "team"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:queue"
private = true
living = true
[[routes]]
id = "lobby"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:queue"
"#
    )
}

#[test]
fn two_routes_posting_to_one_place_where_only_one_is_shown_advisories_are_flagged() {
    let text = advised_text;
    let found = on(&loaded(&text("")), &server(), &Here::default()).notes;
    let shared: Vec<&String> = found
        .iter()
        .filter(|note| note.contains("post to one place"))
        .collect();
    assert_eq!(shared.len(), 1, "{found:?}");
    assert!(
        shared[0].starts_with("The routes team and lobby post to one place, and only one of them is shown the report daily's advisories"),
        "{}",
        shared[0]
    );
    let written = text("");
    let (head, tail) = written.rsplit_once("1566200000000000100").unwrap();
    let apart = on(
        &loaded(&format!("{head}1566200000000000200{tail}")),
        &server(),
        &Here::default(),
    )
    .notes;
    assert!(
        apart.iter().all(|note| !note.contains("post to one place")),
        "{apart:?}"
    );
}

#[test]
fn a_private_route_s_check_is_named_with_its_cost_and_one_vouched_for_as_on_the_operator_s_word() {
    let found = on(&loaded(&advised_text("")), &server(), &Here::default()).notes;
    let checked: Vec<&String> = found
        .iter()
        .filter(|note| note.starts_with("Before each run's first advisory send"))
        .collect();
    assert_eq!(
        checked,
        [&format!(
            "Before each run's first advisory send, herald asks {}, by 3 requests charged to the run's budget, once a run for its place; where anyone more may read it, or herald cannot tell, the route team is sent \"{}\" in place of the report daily's advisories.",
            herald_dest_discord::ASKS,
            herald_core::WITHHELD
        )],
        "{found:?}"
    );
    let hooked = advised_text("")
        .replace(
            "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
            "webhook = { secret = \"HERALD_DISCORD_WEBHOOK\" }",
        )
        .replace("channel = \"1566200000000000100\"\n", "");
    for (vouch, wanted) in [
        (
            "",
            "unless you vouch for its place with vouch_unchecked = true.",
        ),
        (
            "vouch_unchecked = true\n",
            "on the operator's word alone, as it says vouch_unchecked = true",
        ),
    ] {
        let text = hooked.replace("private = true\n", &format!("private = true\n{vouch}"));
        let found = on(&loaded(&text), &server(), &Here::default()).notes;
        assert!(
            found
                .iter()
                .any(|note| note.contains(wanted) && note.contains(herald_dest_discord::UNCHECKED)),
            "{vouch}: {found:?}"
        );
    }
    let markers = advised_text("").replace("kind = \"sqlite\"", "kind = \"markers\"");
    let found = on(&loaded(&markers), &server(), &Here::default()).notes;
    assert!(
        found.iter().any(|note| note.starts_with(
            "The route team keeps a living post and says private = true, but the store keeps no names"
        )),
        "{found:?}"
    );
    let posted = advised_text("").replace("living = true\n", "");
    let found = on(&loaded(&posted), &server(), &Here::default()).notes;
    assert!(
        found.iter().any(|note| note.starts_with(
            "The route team says private = true, but its post is not living, so it is sent"
        )),
        "{found:?}"
    );
}

#[test]
fn a_rule_planning_re_runs_says_whether_an_apply_can_re_run_each_source_s_runs_and_why_not() {
    let text = |(store, kind, keys): (&str, &str, &str)| {
        format!(
            r#"version = 1
[store]
kind = "{store}"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.graphed]
kind = "{kind}"
repository = "o/graphed"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
{keys}
[views.queue]
mode = "live"
[rules.flaky]
flaky = {{ rerun = true }}
"#
        )
    };
    let applies = |given| -> String {
        on(&loaded(&text(given)), &server(), &Here::default())
            .notes
            .into_iter()
            .find(|note| note.starts_with("herald rerun --apply"))
            .unwrap()
    };
    let write = r#"write = { token = { secret = "HERALD_RERUN" } }"#;
    let both = format!("{write}\nsign_in = {{ client_id = \"Iv23liClient\" }}");
    assert_eq!(
        applies(("sqlite", "github", write)),
        format!(
            "herald rerun --apply cannot re-run the jobs graphed reads: {}.",
            herald_kinds::UNSIGNED
        )
    );
    assert!(applies(("markers", "github", &both)).ends_with(&format!("{}.", herald_flows::UNKEPT)));
    let ready = applies(("sqlite", "github", &both));
    assert!(
        ready.starts_with("herald rerun --apply can re-run the jobs graphed reads, each once: an operator signs in at https://github.com as the client Iv23liClient asking no scope, and must hold write or above on the repository; the call is made with HERALD_RERUN alone"),
        "{ready}"
    );
    let gitlab = applies(("sqlite", "gitlab", &both));
    assert!(
        gitlab.contains("at https://gitlab.com as the client Iv23liClient asking read_api"),
        "{gitlab}"
    );
}

#[test]
fn doctor_says_how_a_view_s_load_folds_and_names_each_team_below_the_least_team() {
    let text = |extra: &str| {
        format!(
            r#"version = 1
[store]
kind = "sqlite"
[sources.graphed]
kind = "github"
repository = "o/graphed"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[teams."o/three"]
members = ["ann", "ben", "cat"]
[teams."o/two"]
members = ["dan", "eve"]
[teams."o/one"]
members = ["fay", "FAY@github.com"]
[views.queue]
{extra}
[reports.daily]
"#
        )
    };
    let note = |extra: &str| {
        on(&loaded(&text(extra)), &server(), &Here::default())
            .notes
            .into_iter()
            .find(|note| note.contains("'s load"))
    };
    assert_eq!(note(""), None);
    let shipped = note("load = {}").unwrap();
    assert!(
        shipped.starts_with("The report daily shows the view queue's load per team and per area, never per person: the review requests open on the rows each surface shows, and the reviews given on them over a window of 1w where every source is read kept"),
        "{shipped}"
    );
    assert!(
        shipped.contains("fewer than 3 members")
            && shipped.contains("folds, and counts no person toward \"other\"")
            && shipped
                .ends_with(" The teams o/one, o/two have fewer than 3 members, so they fold."),
        "{shipped}"
    );
    let raised = note("load = { least_team = 4, window = \"3d\", unknown_teams = true }").unwrap();
    assert!(
        raised.contains("over a window of 3d")
            && raised.contains("unknown_teams = true shows a team, group or role area no [teams.<name>] gives members to as it is: such an area may be one person")
            && raised.ends_with(" The teams o/one, o/three, o/two have fewer than 4 members, so they fold."),
        "{raised}"
    );
    let whole = text("load = {}")
        .replace("[teams.\"o/two\"]\nmembers = [\"dan\", \"eve\"]\n", "")
        .replace(
            "[teams.\"o/one\"]\nmembers = [\"fay\", \"FAY@github.com\"]\n",
            "",
        );
    let whole = on(&loaded(&whole), &server(), &Here::default())
        .notes
        .into_iter()
        .find(|note| note.contains("'s load"))
        .unwrap();
    assert!(whole.ends_with("toward \"other\"."), "{whole}");
}

#[test]
fn doctor_says_a_source_reading_its_forge_s_teams_costs_reaches_private_surfaces_alone_and_needs_a_permission()
 {
    let text = |teams: &str, load: &str| {
        format!(
            r#"version = 1
[store]
kind = "sqlite"
[sources.graphed]
kind = "github"
repository = "o/graphed"
api = "https://ghe.example/api/v3"
teams = "{teams}"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[views.queue]
{load}
[reports.daily]
"#
        )
    };
    let note = |teams: &str, load: &str| {
        on(&loaded(&text(teams, load)), &server(), &Here::default())
            .notes
            .into_iter()
            .find(|note| note.starts_with("The source graphed reads"))
    };
    let said = note("forge", "load = {}").unwrap();
    assert!(
        said.contains("at most 100 teams, each with at most 500 members, by at most 501 requests")
            && said.contains("only a route that says private = true and herald queue with --private or [cli] private")
            && said.contains("read:org")
            && said.contains("GitHub Enterprise Server and GHE.com"),
        "{said}"
    );
    assert_eq!(note("config", "load = {}"), None);
    assert_eq!(note("forge", ""), None);
}

#[test]
fn a_view_reading_the_merge_queue_names_its_cost_on_each_forge_and_where_none_is_read() {
    let config = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[sources.graphed]
kind = "github"
repository = "o/graphed"
token = { secret = "HERALD_GITHUB_TOKEN" }
[sources.bare]
kind = "github"
repository = "o/bare"
[sources.lab]
kind = "gitlab"
repository = "o/lab"
api = "https://gitlab.example/api/v4"
[sources.fj]
kind = "forgejo"
repository = "o/fj"
api = "https://codeberg.org/api/v1"
[views.queue]
sources = ["graphed", "bare", "lab", "fj"]
queue = true
[reports.daily]
"#,
    );
    let notes: Vec<String> = on(&config, &server(), &Here::default())
        .notes
        .into_iter()
        .filter(|note| note.starts_with("The report daily"))
        .collect();
    let ends: Vec<&str> = notes
        .iter()
        .map(|note| note.rsplit_once("; ").map_or("", |(_, end)| end))
        .collect();
    assert_eq!(
        ends,
        [
            "it reads each pull request's merge queue entry with the pull request.",
            "it reads no merge queue entry: GitHub's merge queue is read through GraphQL, with a token, which this source does not read with.",
            "a project whose trains GitLab does not let its token read, as without Premium, shows none.",
            "it reads no merge queue entry: Forgejo and Gitea have no merge queue.",
        ]
    );
    assert!(notes[2].contains("; it reads its merge trains by at most 1 more requests a round; "));
}

#[test]
fn a_release_s_header_names_what_it_reads_what_it_costs_and_what_its_token_needs() {
    let text = |release: &str| {
        format!(
            r#"version = 1
[store]
kind = "sqlite"
[sources.graphed]
kind = "github"
repository = "o/graphed"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[views.queue]
release = {release}
[reports.daily]
"#
        )
    };
    let note = |release: &str| -> Option<String> {
        on(&loaded(&text(release)), &server(), &Here::default())
            .notes
            .into_iter()
            .find(|note| note.contains("open milestones"))
    };
    let headed = note("{ milestone = \"v2.*\" }").unwrap();
    assert!(
        headed.starts_with("The report daily heads the view queue's release with its open milestones the milestone selector matches: one request a round for each repository it shows a pull request of the release in, and for a one-repository source's own repository, at most 20 repositories, those past it named unread; on GitLab one more request for each milestone matched to count its issues, at most 10 a repository."),
        "{headed}"
    );
    assert!(
        headed.contains("Issues: read") && headed.contains("never shown as having no milestone"),
        "{headed}"
    );
    assert_eq!(note("{ milestone = \"v2.*\", header = false }"), None);
    assert_eq!(note("{ label = \"ship\" }"), None);
}
